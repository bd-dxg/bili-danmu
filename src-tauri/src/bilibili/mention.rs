//! 弹幕里的 @提及：从报文里取出「被 @ 的人」
//!
//! B 站与抖音**不同**：@提及不在正文里。服务端把名字单独下发在
//! `DANMU_MSG.info[0][15].extra`（`extra` 是**二次编码的 JSON 字符串**），实测样本：
//!
//! ```text
//! "show_reply":true,"reply_mid":437798391,"reply_uname":"两米的吉三日","reply_type_enum":1
//! ```
//!
//! 正文里那部分已被服务端剥掉（只剩一个前导空格），所以只能读字段——没有 @ 的弹幕
//! `reply_uname` 整个字段缺失、`reply_mid` 为 0；`show_reply` 在两种情况**都是 true**，
//! 不能拿它当判据。
//!
//! 渲染与朗读都据此工作：弹幕窗显示「@名字 正文」，朗读念「发送者对名字说 正文」
//! （组装在 `crate::tts::text::build_text`）。

/// 被提及者名字的最大字数（昵称上限：再长就不是昵称，而是把整句正文当成了名字）
const MENTION_NAME_MAX_CHARS: usize = 30;

/// 从 `info[0]` 里取出被 @ 的人（没人被 @ 时返回 None）
///
/// 取 `info[0][15].extra`：实测 `extra` 是 JSON 字符串（值是二次编码的 JSON 文本），
/// 故先取字符串再解析一次；个别版本若直接给对象也能认。
/// 名字缺失、为空或超过 [`MENTION_NAME_MAX_CHARS`] 字时一律按「没有提及」处理。
pub(crate) fn parse_mention(head: &[serde_json::Value]) -> Option<String> {
    let slot = head.get(15)?;
    let extra = slot.get("extra").unwrap_or(slot);
    // `extra` 实测是二次编码的 JSON 字符串，先解析一次；个别版本直接给对象也能认
    let parsed: serde_json::Value;
    let obj = match extra {
        serde_json::Value::String(s) => {
            parsed = serde_json::from_str(s).ok()?;
            &parsed
        }
        other => other,
    };
    let name = obj.get("reply_uname")?.as_str()?.trim();
    if name.is_empty() || name.chars().count() > MENTION_NAME_MAX_CHARS {
        return None;
    }
    Some(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 造一个 `info[0]`：前 15 位填空，第 15 位放提及槽（实测形态）
    fn head_with_extra(extra: serde_json::Value) -> Vec<serde_json::Value> {
        let mut head = vec![json!(0); 15];
        head.push(json!({ "extra": extra, "mode": 0, "show_player_type": 0 }));
        head
    }

    #[test]
    fn 取出被提及的人() {
        // 实测原文：extra 是二次编码的 JSON 字符串
        let extra =
            json!(r#"{"show_reply":true,"reply_mid":437798391,"reply_uname":"两米的吉三日","reply_type_enum":1}"#);
        assert_eq!(parse_mention(&head_with_extra(extra)), Some("两米的吉三日".into()));
    }

    #[test]
    fn 没有提及时不认() {
        // 实测：没有 @ 的弹幕 reply_uname 整个字段缺失、reply_mid 为 0（show_reply 仍是 true）
        let extra = json!(r#"{"show_reply":true,"reply_mid":0,"reply_type_enum":0}"#);
        assert_eq!(parse_mention(&head_with_extra(extra)), None);
        // 空名字同样按没有提及处理
        let empty = json!(r#"{"show_reply":true,"reply_mid":1,"reply_uname":"  "}"#);
        assert_eq!(parse_mention(&head_with_extra(empty)), None);
    }

    #[test]
    fn 报文字段缺失或畸形时不算提及() {
        assert_eq!(parse_mention(&head_with_extra(json!("不是 JSON"))), None);
        assert_eq!(parse_mention(&[]), None, "缺少 info[0][15]");
        // 名字过长（不是昵称，而是把整句正文当成了名字）
        let long = format!(r#"{{"reply_uname":"{}"}}"#, "字".repeat(40));
        assert_eq!(parse_mention(&head_with_extra(json!(long))), None);
    }

    #[test]
    fn extra_直接给对象也认() {
        // 容错：个别版本可能不下发字符串而直接给对象
        let extra = json!({ "reply_uname": "两米的吉三日" });
        assert_eq!(parse_mention(&head_with_extra(extra)), Some("两米的吉三日".into()));
    }
}
