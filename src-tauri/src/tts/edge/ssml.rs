//! SSML 构造：正文转义、控制字符清理、音色名白名单
//!
//! 协议与收流在父模块 `edge.rs`；这里只有纯字符串处理，不发网络请求。

use super::voices::VOICES;

/// 拼 SSML；正文需 XML 转义，控制字符替换为空格（否则服务端报错）
pub fn build_ssml(voice: &str, rate_pct: i32, volume_pct: i32, text: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
