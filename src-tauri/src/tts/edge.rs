//! Edge TTS（微软 Edge 浏览器「朗读」）在线语音合成客户端
//!
//! 直连 `speech.platform.bing.com` 的 Read Aloud WebSocket，无 Python / 无 SDK 依赖：
//!   1. 握手：URL 带 DRM 令牌 `Sec-MS-GEC` = SHA256(向下取整到 5 分钟的时间戳 + 固定客户端令牌)
//!   2. 发 `speech.config` 声明输出格式（实测服务端不支持 raw PCM，只能 MP3）
//!   3. 发 SSML 请求
//!   4. 收文本帧（turn.start / response / turn.end）与二进制帧（音频分片）
//!
//! 协议来源与实测结论见 `Task/findings.md`；`scripts/edge-tts-probe.ps1` 可单独复现验证。

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

/// 微软固定客户端令牌（Edge 浏览器内置，公开常量）
const TRUSTED_CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
/// DRM 版本串，格式 `1-<Chromium 完整版本号>`；跟随 Edge 更新即可
const SEC_MS_GEC_VERSION: &str = "1-143.0.3650.75";
/// Windows FILETIME 纪元（1601-01-01）相对 Unix 纪元的秒数
const WIN_EPOCH: i64 = 11_644_473_600;
/// 输出格式：服务端只支持 MP3 系列，raw PCM 会被直接断连（已实测）
const OUTPUT_FORMAT: &str = "audio-24khz-48kbitrate-mono-mp3";
/// 收流阶段超时（整条合成的总超时由调用方 `tts::SYNTH_TIMEOUT` 包住，含建连）
const RECV_TIMEOUT: Duration = Duration::from_secs(20);
/// Edge 朗读页面来源，服务端会校验
const ORIGIN: &str = "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0";

/// 内置中文音色（离线兜底 + 列表拉取前的初值；拉取成功后会换成微软返回的完整中文列表）
///
/// 显示名统一「人名·性别（口音）」，**一律用简体字**（繁体看着别扭，且配置界面对不上）。
pub const VOICES: &[(&str, &str)] = &[
    ("zh-CN-XiaoxiaoNeural", "晓晓·女声"),
    ("zh-CN-XiaoyiNeural", "晓伊·女声"),
    ("zh-CN-YunxiNeural", "云希·男声"),
    ("zh-CN-YunjianNeural", "云健·男声"),
    ("zh-CN-YunyangNeural", "云扬·男声（新闻腔）"),
    ("zh-CN-YunxiaNeural", "云夏·男声（童声）"),
    ("zh-CN-liaoning-XiaobeiNeural", "晓北·女声（东北话）"),
    ("zh-CN-shaanxi-XiaoniNeural", "晓妮·女声（陕西话）"),
    ("zh-HK-HiuGaaiNeural", "晓佳·女声（粤语）"),
    ("zh-HK-HiuMaanNeural", "晓曼·女声（粤语）"),
    ("zh-HK-WanLungNeural", "云龙·男声（粤语）"),
    ("zh-TW-HsiaoChenNeural", "晓臻·女声（台湾）"),
    ("zh-TW-HsiaoYuNeural", "晓雨·女声（台湾）"),
    ("zh-TW-YunJheNeural", "云哲·男声（台湾）"),
];

/// 内置音色列表（(音色 ID, 显示名)）
pub fn builtin_voices() -> Vec<(String, String)> {
    VOICES
        .iter()
        .map(|(id, label)| ((*id).to_string(), (*label).to_string()))
        .collect()
}

/// `voices/list` 返回的音色条目（只取需要的字段）
#[derive(Debug, Deserialize)]
struct VoiceEntry {
    #[serde(rename = "ShortName")]
    short_name: String,
    #[serde(rename = "Gender", default)]
    gender: String,
    #[serde(rename = "Locale", default)]
    locale: String,
}

/// 拉取微软的中文音色列表（当前 14 个），返回已排好序的 (音色 ID, 显示名)
///
/// 只保留 `zh-` 前缀：外语音色念中文会带口音，列表也没必要铺 300+ 项。
/// 排序：普通话 → 中文方言 → 粤语/台湾。
pub async fn fetch_voices() -> Result<Vec<(String, String)>, String> {
    let url = format!(
        "https://speech.platform.bing.com/consumer/speech/synthesize/readaloud/voices/list\
         ?trustedclienttoken={TRUSTED_CLIENT_TOKEN}"
    );
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| format!("构造音色请求失败: {e}"))?;
    let mut request = client
        .get(url)
        .header("Sec-MS-GEC", sec_ms_gec())
        .header("Sec-MS-GEC-Version", SEC_MS_GEC_VERSION);
    for (key, value) in edge_headers() {
        request = request.header(key, value);
    }
    let entries: Vec<VoiceEntry> = request
        .send()
        .await
        .map_err(|e| format!("请求音色列表失败（网络不可达或被限流）: {e}"))?
        .json()
        .await
        .map_err(|e| format!("解析音色列表失败: {e}"))?;
    if entries.is_empty() {
        return Err("微软返回的音色列表为空".into());
    }

    let mut voices: Vec<(String, String)> = entries
        .into_iter()
        .filter(|e| e.short_name.to_ascii_lowercase().starts_with("zh-"))
        .map(|e| {
            let label = label_for(&e.short_name, &e.gender, &e.locale);
            (e.short_name, label)
        })
        .collect();
    if voices.is_empty() {
        return Err("微软返回的音色列表里没有中文音色".into());
    }
    voices.sort_by(|a, b| {
        voice_group(&a.0)
            .cmp(&voice_group(&b.0))
            .then_with(|| a.0.cmp(&b.0))
    });
    voices.dedup_by(|a, b| a.0 == b.0);
    Ok(voices)
}

/// 音色排序分组：0 普通话 / 1 中文方言 / 2 粤语与台湾 / 3 其他语言
fn voice_group(short_name: &str) -> u8 {
    let lower = short_name.to_ascii_lowercase();
    if lower.starts_with("zh-cn") {
        // zh-CN-XxxNeural 是普通话；zh-CN-liaoning-Xxx 这类多一段，是方言
        if lower.matches('-').count() == 2 {
            0
        } else {
            1
        }
    } else if lower.starts_with("zh-") {
        2
    } else {
        3
    }
}

/// 显示名：内置表里有中文名的用中文名，其余用「性别·语言」
fn label_for(short_name: &str, gender: &str, locale: &str) -> String {
    if let Some((_, nice)) = VOICES.iter().find(|(id, _)| *id == short_name) {
        return (*nice).to_string();
    }
    let gender = match gender {
        "Female" => "女声",
        "Male" => "男声",
        _ => "其他",
    };
    format!("{gender}·{locale}")
}

/// Edge 服务端校验必需的公共请求头（WS 与 voices/list 共用）
fn edge_headers() -> Vec<(&'static str, String)> {
    vec![
        ("Origin", ORIGIN.to_string()),
        ("Pragma", "no-cache".to_string()),
        ("Cache-Control", "no-cache".to_string()),
        ("User-Agent", USER_AGENT.to_string()),
        ("Cookie", format!("muid={};", random_hex(16, true))),
    ]
}

/// Edge TTS WebSocket 连接（原生 TLS）
type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// 建立一条 Edge TTS WebSocket（DRM 令牌 + 服务端校验必需的请求头）
///
/// 每条合成单独建连：服务端**不接受**同一条连接发多轮（实测第二轮直接 RST 10054），故不复用。
async fn connect() -> Result<Ws, String> {
    let url = format!(
        "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1\
         ?TrustedClientToken={TRUSTED_CLIENT_TOKEN}\
         &ConnectionId={}\
         &Sec-MS-GEC={}\
         &Sec-MS-GEC-Version={SEC_MS_GEC_VERSION}",
        random_hex(16, false),
        sec_ms_gec(),
    );
    let mut req = url
        .into_client_request()
        .map_err(|e| format!("构造 Edge TTS 请求失败: {e}"))?;
    {
        let headers = req.headers_mut();
        for (key, value) in edge_headers() {
            if let Ok(value) = HeaderValue::from_str(&value) {
                headers.insert(key, value);
            }
        }
    }
    let (ws, _) = tokio_tungstenite::connect_async(req)
        .await
        .map_err(|e| format!("Edge TTS 连接失败（网络不可达或被限流）: {e}"))?;
    Ok(ws)
}

/// speech.config 帧：声明输出格式（每轮合成前都要先发）
fn speech_config_frame(now: &str) -> String {
    format!(
        "X-Timestamp:{now}\r\n\
         Content-Type:application/json; charset=utf-8\r\n\
         Path:speech.config\r\n\r\n\
         {{\"context\":{{\"synthesis\":{{\"audio\":{{\"metadataoptions\":{{\
         \"sentenceBoundaryEnabled\":\"false\",\"wordBoundaryEnabled\":\"false\"}},\
         \"outputFormat\":\"{OUTPUT_FORMAT}\"}}}}}}}}\r\n"
    )
}

/// ssml 请求帧
fn ssml_frame(now: &str, ssml: &str) -> String {
    format!(
        "X-RequestId:{}\r\n\
         Content-Type:application/ssml+xml\r\n\
         X-Timestamp:{now}Z\r\n\
         Path:ssml\r\n\r\n{ssml}",
        random_hex(16, false),
    )
}

/// 在一条连接上合成一轮（发送 speech.config + ssml 并收流）
async fn synth_turn(
    ws: &mut Ws,
    voice: &str,
    rate_pct: i32,
    volume_pct: i32,
    text: &str,
) -> Result<Vec<u8>, String> {
    let now = date_string();
    ws.send(Message::Text(speech_config_frame(&now).into()))
        .await
        .map_err(|e| format!("发送 speech.config 失败: {e}"))?;

    let ssml = build_ssml(voice, rate_pct, volume_pct, text);
    ws.send(Message::Text(ssml_frame(&now, &ssml).into()))
        .await
        .map_err(|e| format!("发送 SSML 失败: {e}"))?;

    recv_turn(ws).await
}

/// 收一轮合成的音频，直到 turn.end
async fn recv_turn(ws: &mut Ws) -> Result<Vec<u8>, String> {
    // 收流：文本帧判结束、二进制帧抠音频（[2 字节大端 header 长度][header][\r\n][音频]）
    let mut audio: Vec<u8> = Vec::new();
    let deadline = tokio::time::Instant::now() + RECV_TIMEOUT;
    loop {
        let next = tokio::time::timeout_at(deadline, ws.next())
            .await
            .map_err(|_| "Edge TTS 响应超时".to_string())?;
        let Some(msg) = next else { break };
        match msg.map_err(|e| format!("Edge TTS 连接异常: {e}"))? {
            Message::Text(t) => {
                if t.as_str().contains("Path:turn.end") {
                    break;
                }
            }
            Message::Binary(b) => {
                if b.len() < 4 {
                    continue;
                }
                let header_len = u16::from_be_bytes([b[0], b[1]]) as usize;
                if header_len < 2 || header_len + 2 > b.len() {
                    continue;
                }
                let header = String::from_utf8_lossy(&b[2..header_len]);
                if !header.contains("Path:audio") {
                    continue;
                }
                let body = &b[header_len + 2..];
                if !body.is_empty() {
                    audio.extend_from_slice(body);
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    if audio.is_empty() {
        return Err("Edge TTS 未返回音频（音色名可能无效，或被服务端限流）".into());
    }
    Ok(audio)
}

/// 合成一段文本为 MP3 字节（每条新建连接）；失败返回可读原因（调用方只记录，不重试，避免刷屏）
///
/// `rate_pct` / `volume_pct` 为相对基准的百分比偏移（如 -20 表示 -20%）。
pub async fn synthesize(
    voice: &str,
    rate_pct: i32,
    volume_pct: i32,
    text: &str,
) -> Result<Vec<u8>, String> {
    let mut ws = connect().await?;
    synth_turn(&mut ws, voice, rate_pct, volume_pct, text).await
}

/// 拼 SSML；正文需 XML 转义，控制字符替换为空格（否则服务端报错）
fn build_ssml(voice: &str, rate_pct: i32, volume_pct: i32, text: &str) -> String {
    let body = xml_escape(&strip_control_chars(text));
    let voice = safe_voice(voice);
    format!(
        "<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='en-US'>\
         <voice name='{voice}'>\
         <prosody pitch='+0Hz' rate='{rate_pct:+}%' volume='{volume_pct:+}%'>{body}</prosody>\
         </voice></speak>"
    )
}

/// 音色名只允许字母/数字/`-`/`_`（真实音色形如 zh-CN-XiaoxiaoNeural）。
///
/// `voice` 来自配置，是 SSML 里唯一没走 `xml_escape` 的插值点：一个 `'` 就能拆坏 name 属性，
/// 故只保留白名单字符；全被过滤掉时回退内置默认音色，而不是留下空 name。
fn safe_voice(voice: &str) -> String {
    let cleaned: String = voice
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if cleaned.is_empty() {
        VOICES[0].0.to_string()
    } else {
        cleaned
    }
}

/// 服务端不接受的控制字符（0-8 / 11-12 / 14-31）替换为空格
fn strip_control_chars(s: &str) -> String {
    s.chars()
        .map(|c| {
            let code = c as u32;
            if code <= 8 || (11..=12).contains(&code) || (14..=31).contains(&code) {
                ' '
            } else {
                c
            }
        })
        .collect()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// DRM 令牌：时间戳切到 Windows FILETIME 纪元、向下取整到 5 分钟、转 100ns 单位后
/// 与固定客户端令牌拼接做 SHA256，取大写 hex
fn sec_ms_gec() -> String {
    let mut ticks = unix_secs() + WIN_EPOCH;
    ticks -= ticks % 300;
    let ticks = ticks * 10_000_000;
    let mut hasher = Sha256::new();
    hasher.update(format!("{ticks}{TRUSTED_CLIENT_TOKEN}").as_bytes());
    hex(&hasher.finalize(), true)
}

/// JavaScript 风格时间串（服务端照 Edge 的格式校验）：
/// `Sat Nov 15 2025 10:00:00 GMT+0000 (Coordinated Universal Time)`
fn date_string() -> String {
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let secs = unix_secs();
    let days = secs.div_euclid(86_400);
    let today = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{} {} {day:02} {year} {:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        WEEKDAYS[(days + 4).rem_euclid(7) as usize],
        MONTHS[(month - 1) as usize],
        today / 3600,
        today % 3600 / 60,
        today % 60,
    )
}

/// Unix 天数 → (年, 月, 日)，Howard Hinnant 的 civil_from_days 算法
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe as i64 + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

fn unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 随机 hex 串（ConnectionId / RequestId / MUID，只需唯一性，不用于安全用途）
fn random_hex(bytes: usize, upper: bool) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hex(&hasher.finalize()[..bytes], upper)
}

fn hex(bytes: &[u8], upper: bool) -> String {
    bytes
        .iter()
        .map(|b| {
            if upper {
                format!("{b:02X}")
            } else {
                format!("{b:02x}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec_ms_gec_is_upper_hex_sha256() {
        let token = sec_ms_gec();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(token.chars().all(|c| !c.is_ascii_lowercase()));
    }

    #[test]
    fn sec_ms_gec_rounds_down_to_five_minutes() {
        // 同一 5 分钟窗口内算两次必须一致（窗口切换瞬间可能不等，用两次快速取样降低概率）
        let first = sec_ms_gec();
        assert_eq!(first, sec_ms_gec());
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn ssml_escapes_and_strips_control_chars() {
        let ssml = build_ssml("zh-CN-XiaoxiaoNeural", -20, 10, "a<b>&c\u{7}d");
        assert!(ssml.contains("a&lt;b&gt;&amp;c d"));
        assert!(ssml.contains("rate='-20%'"));
        assert!(ssml.contains("volume='+10%'"));
        assert!(ssml.contains("<voice name='zh-CN-XiaoxiaoNeural'>"));
    }

    #[test]
    fn voice_name_is_whitelisted() {
        // voice 是 SSML 里唯一没走 xml_escape 的插值点：一个引号就能拆坏 name 属性
        assert_eq!(
            safe_voice("zh-CN-XiaoxiaoNeural'/><x a='1"),
            "zh-CN-XiaoxiaoNeuralxa1"
        );
        // 全被过滤掉 → 回退内置默认音色，而不是生成空 name
        assert_eq!(safe_voice("'\"<>=%$"), VOICES[0].0);
        assert!(build_ssml("zh-CN-XiaoyiNeural", 0, 0, "a")
            .contains("<voice name='zh-CN-XiaoyiNeural'>"));
    }

    #[test]
    fn date_string_shape() {
        let s = date_string();
        assert!(s.ends_with(" GMT+0000 (Coordinated Universal Time)"));
        assert_eq!(s.len(), 62);
    }

    #[test]
    fn voice_groups_sort_mandarin_first() {
        assert_eq!(voice_group("zh-CN-XiaoxiaoNeural"), 0);
        // 多一段语言标签的是方言（东北话 / 陕西话）
        assert_eq!(voice_group("zh-CN-liaoning-XiaobeiNeural"), 1);
        assert_eq!(voice_group("zh-HK-HiuGaaiNeural"), 2);
        assert_eq!(voice_group("zh-TW-HsiaoChenNeural"), 2);
        assert_eq!(voice_group("en-US-EmmaNeural"), 3);
    }

    #[test]
    fn voice_label_prefers_builtin_chinese_name() {
        assert_eq!(
            label_for("zh-CN-XiaoxiaoNeural", "Female", "zh-CN"),
            "晓晓·女声"
        );
        // 内置表里没有的音色退回「性别·语言」
        assert_eq!(label_for("zh-CN-NewVoiceNeural", "Female", "zh-CN"), "女声·zh-CN");
        assert_eq!(label_for("zh-CN-NewVoiceNeural", "Male", "zh-CN"), "男声·zh-CN");
    }

    /// 真实联网拉取音色列表（默认忽略）：验证 voices/list 的 DRM 头、JSON 解析与中文过滤
    /// `cargo test --lib -- --ignored voice_list_fetches_live`
    #[test]
    #[ignore]
    fn voice_list_fetches_live() {
        let voices = tauri::async_runtime::block_on(fetch_voices()).expect("拉取音色列表失败");
        assert!(voices.len() >= 10, "中文音色数量异常: {}", voices.len());
        assert!(
            voices.iter().all(|(id, _)| id.starts_with("zh-")),
            "应只保留 zh- 前缀音色"
        );
        assert_eq!(voices[0].0, "zh-CN-XiaoxiaoNeural", "普通话应排最前");
        assert!(voices.iter().any(|(id, _)| id == "zh-HK-HiuGaaiNeural"));
        println!("拉取到 {} 个中文音色: {voices:?}", voices.len());
    }

    /// 真实联网突发测试：同一进程内连续 10 条、无间隔，统计失败率
    /// （定位「高频时 10054 / 超时」是本机频率问题还是服务端限流）
    /// `cargo test --lib -- --ignored edge_burst_live --nocapture`
    #[test]
    #[ignore]
    fn edge_burst_live() {
        tauri::async_runtime::block_on(async {
            let (mut ok, mut fail) = (0u32, 0u32);
            for i in 1..=10 {
                let start = std::time::Instant::now();
                match synthesize("zh-CN-YunjianNeural", 0, 0, &format!("第{i}条突发测试")).await {
                    Ok(a) => {
                        ok += 1;
                        println!("第 {i:>2} 条: OK   {} 字节  {:?}", a.len(), start.elapsed());
                    }
                    Err(e) => {
                        fail += 1;
                        println!("第 {i:>2} 条: 失败 {e}  {:?}", start.elapsed());
                    }
                }
            }
            println!("--- 成功 {ok} / 失败 {fail} ---");
        });
    }

    /// 真实联网合成（默认忽略）：验证整条握手 + 收流链路，微软改协议时跑这个
    /// `cargo test --lib -- --ignored edge_synthesizes_mp3_live --nocapture`
    #[test]
    #[ignore]
    fn edge_synthesizes_mp3_live() {
        let mp3 = tauri::async_runtime::block_on(synthesize("zh-CN-XiaoxiaoNeural", 0, 0, "测试"))
            .expect("联网合成失败");
        // MP3 帧同步头：11 位全 1
        assert!(mp3.len() > 1000, "音频太短: {} 字节", mp3.len());
        assert_eq!(mp3[0], 0xFF);
        assert_eq!(mp3[1] & 0xE0, 0xE0);
    }
}
