//! Edge TTS 音色表：内置中文兜底列表 + 微软 `voices/list` 拉取与排序
//!
//! 显示名与排序规则见各函数注释；协议与合成在父模块 `edge.rs`。

use super::util::sec_ms_gec;
use super::{edge_headers, SEC_MS_GEC_VERSION, TRUSTED_CLIENT_TOKEN, USER_AGENT};
use serde::Deserialize;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
