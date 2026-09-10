//! Edge TTS（微软 Edge 浏览器「朗读」）在线语音合成客户端
//!
//! 直连 `speech.platform.bing.com` 的 Read Aloud WebSocket，无 Python / 无 SDK 依赖：
//!   1. 握手：URL 带 DRM 令牌 `Sec-MS-GEC` = SHA256(向下取整到 5 分钟的时间戳 + 固定客户端令牌)
//!   2. 发 `speech.config` 声明输出格式（实测服务端不支持 raw PCM，只能 MP3）
//!   3. 发 SSML 请求
//!   4. 收文本帧（turn.start / response / turn.end）与二进制帧（音频分片）
//!
//! 协议来源与实测结论见 `Task/findings.md`；`scripts/edge-tts-probe.ps1` 可单独复现验证。
//!
//! 子模块：音色表见 `voices.rs`，时间与编码工具见 `util.rs`。

use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::Message;

/// 微软固定客户端令牌（Edge 浏览器内置，公开常量）
const TRUSTED_CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
/// DRM 版本串，格式 `1-<Chromium 完整版本号>`；跟随 Edge 更新即可
const SEC_MS_GEC_VERSION: &str = "1-143.0.3650.75";
/// 输出格式：服务端只支持 MP3 系列，raw PCM 会被直接断连（已实测）
const OUTPUT_FORMAT: &str = "audio-24khz-48kbitrate-mono-mp3";
/// 收流阶段超时（整条合成的总超时由调用方 `tts::SYNTH_TIMEOUT` 包住，含建连）
const RECV_TIMEOUT: Duration = Duration::from_secs(20);
/// Edge 朗读页面来源，服务端会校验
const ORIGIN: &str = "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0";

mod ssml;
mod util;
mod voices;

pub use voices::{builtin_voices, fetch_voices};
use ssml::build_ssml;
use util::{date_string, random_hex, sec_ms_gec};

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

/// Edge TTS 服务端点：自己解析 DNS 才能按地址族择优，并记录实际使用的对端地址
const TTS_HOST: &str = "speech.platform.bing.com";
const TTS_PORT: u16 = 443;

/// 建立到 Edge TTS 的 TCP 连接：**IPv4 优先**，全部失败才回退 IPv6
///
/// 不用 `tokio_tungstenite::connect_async`：它按 DNS 返回顺序尝试（Windows 上 IPv6 通常排前），
/// 连上就用，不会因链路劣化换地址族。国内直连微软的 IPv6 路径偶发被 RST——现象是弹幕一路
/// 正常、朗读却整段全挂（收流阶段 10054），故让 IPv4 先试。
async fn dial_tcp() -> Result<(tokio::net::TcpStream, std::net::SocketAddr), String> {
    let addrs = tokio::net::lookup_host((TTS_HOST, TTS_PORT))
        .await
        .map_err(|e| format!("Edge TTS 域名解析失败: {e}"))?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err("Edge TTS 域名无解析结果".into());
    }
    let (v4, v6): (Vec<_>, Vec<_>) = addrs.into_iter().partition(|a| a.is_ipv4());
    let mut last_err = String::from("无可用地址");
    for addr in v4.into_iter().chain(v6) {
        match tokio::net::TcpStream::connect(addr).await {
            Ok(stream) => {
                if !addr.is_ipv4() {
                    eprintln!("[tts] IPv4 不可达，连接回退到 {addr}（IPv6）");
                }
                return Ok((stream, addr));
            }
            Err(e) => last_err = format!("{addr}: {e}"),
        }
    }
    Err(format!("Edge TTS 所有地址均不可达（最后失败 {last_err}）"))
}

/// 建立一条 Edge TTS WebSocket（DRM 令牌 + 服务端校验必需的请求头）
///
/// 每条合成单独建连：服务端**不接受**同一条连接发多轮（实测第二轮直接 RST 10054），故不复用。
/// 返回 WebSocket 与对端地址（便于失败时定位是哪条网络路径）。
async fn connect() -> Result<(Ws, std::net::SocketAddr), String> {
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
    let (tcp, peer) = dial_tcp().await?;
    let connector = native_tls::TlsConnector::new().map_err(|e| format!("初始化 TLS 失败: {e}"))?;
    let (ws, _) = tokio_tungstenite::client_async_tls_with_config(
        req,
        tcp,
        None,
        Some(tokio_tungstenite::Connector::NativeTls(connector)),
    )
    .await
    .map_err(|e| format!("Edge TTS 连接失败（网络不可达或被限流）: {e}"))?;
    Ok((ws, peer))
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
    let (mut ws, peer) = connect().await?;
    synth_turn(&mut ws, voice, rate_pct, volume_pct, text)
        .await
        // 带上对端地址：IPv4/IPv6 哪条链路出问题一眼可见
        .map_err(|e| format!("{e}（对端 {peer}）"))
}

/// DRM 令牌：时间戳切到 Windows FILETIME 纪元、向下取整到 5 分钟、转 100ns 单位后
/// 与固定客户端令牌拼接做 SHA256，取大写 hex
#[cfg(test)]
mod tests {
    use super::*;

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
