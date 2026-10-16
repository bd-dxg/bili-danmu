//! 弹幕表情包：从报文里认出表情，并把正文里的表情片段剥掉
//!
//! 两件事：
//! - [`parse_emote`]：解 `DANMU_MSG.info[0][13]`，拿 `room_`（UP主大表情）/ `official_`（通用表情）/
//!   `upower_`（收藏集装扮）三类表情的唯一号与图片地址
//! - [`strip_bracket_emotes`]：**无表情字段**的老式小表情（`[花]`）只能按正文形态认，
//!   这类还会和文字混在同一条（`今天真好[花]`）、或多个拼成一条（`[比心][比心][比心]`）
//!
//! 实测数据与结论见 `docs/技术说明.md` 的「表情包」：判断依据不是「有没有中括号」，
//! 而是「唯一号非空」或「正文里有 `[名字]` 片段」。

/// 一个 `[名字]` 片段里名字的最大字数（表情名都很短，最长的收藏集名约 20 字）
///
/// 超过这个长度的中括号文本按普通文字处理，避免把 `[这是一段很长的说明]` 剥掉。
const EMOTE_NAME_MAX_CHARS: usize = 30;

/// 解析 `info[0][13]` 的表情信息 → `Emote`
///
/// 无表情时该位是字符串 `"{}"`（`as_object()` 直接返回 None）；唯一号缺失时也返回 None——
/// 宁可按普通文字弹幕渲染，也不能丢正文。图片地址允许为空。
pub(crate) fn parse_emote(v: &serde_json::Value) -> Option<crate::bilibili::event::Emote> {
    let o = v.as_object()?;
    let unique = o.get("emoticon_unique")?.as_str()?.trim();
    if unique.is_empty() {
        return None;
    }
    // 图片地址允许为空：现在渲染只用表情名（补中括号），地址只用于日志；
    // 早期版本要求地址非空，结果把「有唯一号但没地址」的表情（如某些动态表情）漏成普通文字
    let url = o.get("url").and_then(|u| u.as_str()).unwrap_or("").trim();
    Some(crate::bilibili::event::Emote {
        unique: unique.to_string(),
        url: normalize_url(url),
        width: o.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as u32,
        height: o.get("height").and_then(|h| h.as_u64()).unwrap_or(0) as u32,
        bulge: o.get("bulge_display").and_then(|b| b.as_i64()).unwrap_or(0) == 1,
    })
}

/// 剥离正文里的 `[名字]` 表情片段，返回（剩余文字，是否剥到过表情片段）
///
/// 服务端对老式小表情不给表情字段，只能按正文形态认，而且**一条弹幕可能混着文字**
/// （实测「今天真好[花]」）—— 这类要剥掉表情、只念剩下的文字。
/// 畸形中括号（未闭合、内部还有 `[`、名字为空或过长）一律原样保留，不得剥坏正文。
///
/// 代价（已写入文档）：用户手打的方括号文字（如「价格[10]元」）也会被当表情剥掉。
pub(crate) fn strip_bracket_emotes(content: &str) -> (String, bool) {
    let chars: Vec<char> = content.chars().collect();
    let mut out = String::with_capacity(content.len());
    let mut had = false;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '[' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // 找最近的 `]`，其间不允许出现 `[`
        let mut j = i + 1;
        while j < chars.len() && chars[j] != ']' && chars[j] != '[' {
            j += 1;
        }
        let well_formed = j < chars.len()
            && chars[j] == ']'
            && j > i + 1
            && j - i - 1 <= EMOTE_NAME_MAX_CHARS;
        if well_formed {
            had = true;
            i = j + 1;
        } else {
            // 畸形：从这里开始整段当普通文字（继续往后找会把 `[a[b]c]` 这类剥坏）
            out.extend(chars[i..].iter());
            break;
        }
    }
    (out, had)
}

/// 表情图地址归一化：协议相对（`//`）与明文 `http://` 都升到 `https://`
///
/// 实测：老式小表情给 `//i0.hdslb.com/…`，通用表情（「冲鸭」）给 `http://i0.hdslb.com/…`；
/// 同一地址 https 也返回 200。渲染已不用图（改补中括号），这里只为日志与后续需要时保持一致。
fn normalize_url(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("//") {
        format!("https://{rest}")
    } else if let Some(rest) = url.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn 解析表情对象() {
        // 实测形态：UP主大表情（room_，bulge_display=1，162×162）
        let e = parse_emote(&json!({
            "bulge_display": 1,
            "emoticon_unique": "room_22637261_34519",
            "height": 162,
            "is_dynamic": 1,
            "url": "https://i0.hdslb.com/bfs/garb/92855a2c.png",
            "width": 162
        }))
        .expect("应解析出表情");
        assert_eq!(e.unique, "room_22637261_34519");
        assert_eq!((e.width, e.height), (162, 162));
        assert!(e.bulge, "bulge_display=1 应标记为大表情");
    }

    #[test]
    fn 无字段或空唯一号不算表情() {
        // 无表情时该位是字符串 "{}"（不是空对象），as_object() 直接 None
        assert!(parse_emote(&json!("{}")).is_none());
        assert!(parse_emote(&json!({})).is_none(), "缺唯一号");
        assert!(parse_emote(&json!({ "emoticon_unique": "" })).is_none(), "空唯一号");
        assert!(parse_emote(&json!({ "emoticon_unique": null })).is_none());
    }

    #[test]
    fn 只有唯一号没图片地址也算表情() {
        // 早期版本要求地址非空，把这类表情漏成普通文字、还被朗读
        let e = parse_emote(&json!({
            "bulge_display": 1,
            "emoticon_unique": "upower_[喝彩]",
            "height": 20,
            "width": 20
        }))
        .expect("有唯一号就应认成表情");
        assert_eq!(e.unique, "upower_[喝彩]");
        assert_eq!(e.url, "");
    }

    #[test]
    fn 表情图地址统一升到https() {
        for (raw, want) in [
            ("//i0.hdslb.com/bfs/emote/x.png", "https://i0.hdslb.com/bfs/emote/x.png"),
            ("http://i0.hdslb.com/bfs/live/y.png", "https://i0.hdslb.com/bfs/live/y.png"),
            ("https://i0.hdslb.com/bfs/garb/z.png", "https://i0.hdslb.com/bfs/garb/z.png"),
        ] {
            let e = parse_emote(&json!({ "emoticon_unique": "emoji_1", "url": raw })).expect("应解析出表情");
            assert_eq!(e.url, want, "{raw} 应归一化为 {want}");
        }
    }

    #[test]
    fn 剥离中括号表情片段() {
        let cases: [(&str, &str, bool); 13] = [
            ("[花]", "", true),
            ("[比心][比心][比心]", "", true),
            ("[喝彩][喝彩][喝彩][喝彩]", "", true),
            // 混着文字（实测形态）：只剥表情，文字留下
            ("今天真好[花]", "今天真好", true),
            ("[花]今天真好", "今天真好", true),
            ("今天[花]真好", "今天真好", true),
            ("今天真好[花][花]", "今天真好", true),
            ("[2026嘉然的画册_顿悟]好", "好", true),
            // 普通文字一律不动
            ("普通弹幕", "普通弹幕", false),
            ("[前半", "[前半", false),
            ("后半]", "后半]", false),
            ("[a[b]c]", "[a[b]c]", false),
            ("[]", "[]", false),
        ];
        for (input, want, had_want) in cases {
            let (got, had) = strip_bracket_emotes(input);
            assert_eq!(got, want, "「{input}」剥离后应为「{want}」");
            assert_eq!(had, had_want, "「{input}」是否含表情片段");
        }
        // 名字过长（> 30 字）不当表情，避免剥掉长说明文字
        let long = format!("[{}]", "字".repeat(40));
        assert_eq!(strip_bracket_emotes(&long), (long.clone(), false));
    }
}
