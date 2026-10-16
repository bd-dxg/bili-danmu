//! 朗读前清洗：丢掉噪声字符与链接、折叠连续重复字符、规整空白
//!
//! 从 `text.rs` 拆出（那边代码行数超过 300）：清洗只做纯字符处理，
//! 与「筛选 + 组装文案」互不相干，且两块各有自己的单测。
//!
//! `NAMED_CHARS` 是逐字实测得出的（见其注释），改动前先用
//! `scripts/edge-tts-probe.ps1` 复测 Δ 字节数。

/// 同一字符连续重复的最大保留次数（「哈哈哈哈哈哈哈」只读三个）
const MAX_REPEAT: u32 = 3;

/// 朗读前清洗（prd.md §24）：丢掉噪声字符与链接、折叠连续重复字符、规整空白
pub(super) fn clean_for_speech(text: &str) -> String {
    collapse_repeats(&strip_noise(text))
        .split_whitespace()
        .filter(|token| !is_link(token))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 只剔除噪声字符（昵称这类短文本用：不该折叠重复字符，也不该丢链接）
pub(super) fn strip_noise(text: &str) -> String {
    text.chars().filter(|ch| !is_noise(*ch)).collect()
}

/// 同一字符连续超过 MAX_REPEAT 次的部分丢掉（「哈哈哈哈哈哈哈」只读三个）
fn collapse_repeats(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev = '\0';
    let mut repeat = 0u32;
    for ch in text.chars() {
        if ch == prev {
            repeat += 1;
            if repeat > MAX_REPEAT {
                continue;
            }
        } else {
            prev = ch;
            repeat = 1;
        }
        out.push(ch);
    }
    out
}

/// 会被引擎念出「名字」而不是当作内容的字符。
///
/// 本机对 zh-CN-XiaoxiaoNeural 逐字实测得出（`scripts/edge-tts-probe.ps1`，Δ 字节数见 `Task/findings.md`）：
/// - `_`/`＿`→下划线、`\`/`＼`→反斜杠、`#`/`＃`→井号、`*`/`＊`→星号、`=`/`＝`→等号
/// - `%`/`％`→百分号、`$`/`＄`→美元、`@`/`＠`→at、`°`→度
/// - `ω`→欧米伽、`ε`→epsilon、`Σ`→sigma、`∀`→任意、`ψ`/`π`→字母名（颜文字常客）
///
/// 实测全角与半角行为完全一致，故两套一并列出。实测发音**有意义**、故保留的：
/// `×`→乘以、`÷`→除以、`±`→正负、`○`→零；实测**不出声**、无需处理的：
/// `~ ～ ! ? - . , / | ^ < > [ ] ( ) { } : ; · ・ … ※ ★ ☆ □ △ ▲ ● ◇ ◆ ▁ ﹏ ╯ ╰ ﾟ ｀`
const NAMED_CHARS: &str = "_\\#*=%@$°＿＼＃＊＝％＄＠ωεΣ∀ψπ";

/// 朗读前丢弃的字符：控制字符、emoji / 图形符号、以及会被念出名字的字符
fn is_noise(ch: char) -> bool {
    if NAMED_CHARS.contains(ch) {
        return true;
    }
    let code = ch as u32;
    code < 0x20
        || code == 0x7F
        || code == 0x200D
        || code == 0xFFFD
        || (0xFE00..=0xFE0F).contains(&code)
        || (0x2190..=0x21FF).contains(&code)
        || (0x2600..=0x27BF).contains(&code)
        || (0x2B00..=0x2BFF).contains(&code)
        || (0x1F000..=0x1FAFF).contains(&code)
}

/// 网址不值得念，整段丢掉
fn is_link(token: &str) -> bool {
    let lower = token.to_lowercase();
    lower.starts_with("http")
        || lower.starts_with("www.")
        || lower.contains("://")
        || lower.contains(".com/")
        || lower.contains(".cn/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_repeated_characters() {
        assert_eq!(clean_for_speech("哈哈哈哈哈哈哈哈"), "哈哈哈");
        assert_eq!(clean_for_speech("6"), "6");
        assert_eq!(clean_for_speech("6666666到飞起"), "666到飞起");
    }

    #[test]
    fn strips_emoji_and_links_and_whitespace() {
        assert_eq!(clean_for_speech("主播好棒😄🎉"), "主播好棒");
        assert_eq!(clean_for_speech("看这里 https://b23.tv/abc 谢谢"), "看这里 谢谢");
        assert_eq!(clean_for_speech("  多个   空格  "), "多个 空格");
    }

    #[test]
    fn strips_symbols_the_engine_would_name_out_loud() {
        // `_` 会被念成「下划线」：只忽略这个字符，整条弹幕照读
        assert_eq!(clean_for_speech("主播_好棒"), "主播好棒");
        assert_eq!(clean_for_speech("_顶_你_"), "顶你");
        assert_eq!(clean_for_speech("C_语言 ＃1 星＊号"), "C语言 1 星号");
        // 只由这类符号组成 → 确实没东西可念，交由上层丢弃
        assert!(clean_for_speech("_＿_＼").is_empty());
        // 实测不出声的符号保留（波浪号是语气延伸，不该删）
        assert_eq!(clean_for_speech("你好~!!"), "你好~!!");
        // 颜文字里的希腊字母会被念成字母名（「欧米伽」），度号会被念成「度」
        assert_eq!(clean_for_speech("(╯°□°)╯ε"), "(╯□)╯");
    }

    #[test]
    fn symbols_do_not_break_repeat_collapsing() {
        assert_eq!(clean_for_speech("哈哈哈_哈哈哈"), "哈哈哈");
    }
}
