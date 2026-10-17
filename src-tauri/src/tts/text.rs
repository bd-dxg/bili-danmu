//! 朗读文案：筛选判定 + 组装（清洗见 `clean.rs`）
//!
//! 全文只有纯字符串处理，不碰队列与网络，便于单测覆盖边界。
//! 筛选语义与前端 Overlay 的显示筛选一致（见 `matches_filter` 注释）。

use super::clean::{clean_for_speech, strip_noise};
use crate::bilibili::emote::strip_bracket_emotes;
use crate::bilibili::event::{Backing, BackingKind, Danmaku, Welcome};
use crate::config::{DanmakuFilter, TtsConfig};

/// 醒目留言正文的朗读上限（字）：SC 正文最长 200 字（≈ 40s 音频），
/// 不截断的话一条留言会把后面所有弹幕与打赏的朗读一起堵住
const MAX_SUPER_CHAT_LEN: usize = 50;

/// 朗读筛选判定：与前端 Overlay 的显示筛选同语义
/// （身份规则之间是「或」，全部关闭 = 不过滤；敏感词屏蔽独立叠加且不豁免）
pub(super) fn matches_filter(d: &Danmaku, f: &DanmakuFilter) -> bool {
    if f.enable_sensitive {
        let content = d.content.to_lowercase();
        if f.sensitive_words
            .iter()
            .any(|w| !w.is_empty() && content.contains(&w.to_lowercase()))
        {
            return false;
        }
    }
    if !(f.enable_guard_admin || f.enable_medal || f.enable_wealth) {
        return true;
    }
    (f.enable_guard_admin && (d.is_admin || d.guard_level.unwrap_or(0) >= 1))
        || (f.enable_medal && d.medal_level.unwrap_or(0) > 0)
        || (f.enable_wealth && d.wealth_level.unwrap_or(0) >= f.wealth_min)
}

/// 是否含表情弹幕：报文带表情字段，或正文里有 `[名字]` 片段（老式小表情不给字段）
pub(super) fn is_emote(d: &Danmaku) -> bool {
    d.emote.is_some() || strip_bracket_emotes(&d.content).1
}

/// 组装朗读文案：可选身份前缀 + 可选用户名 + 清洗后的正文
///
/// 正文没有可读内容（纯 emoji / 纯标点 / 纯链接）时整条返回空，
/// 不能只剩下「舰长 小明说」这种没内容的空壳。
/// `max_len` 只约束正文：前缀与用户名是固定少量字，不该挤掉弹幕本身。
pub(super) fn build_text(d: &Danmaku, config: &TtsConfig) -> String {
    // 表情部分默认不念：
    // - 报文带表情字段的（room_/official_/upower_）：正文就是表情名（「生气」「冲鸭」）→ 整条不念
    // - 不带字段的老式小表情（「[花]」）：正文可能混着文字（实测「今天真好[花]」）→ 剥掉 `[名字]`
    //   片段、只念剩下的文字；剥完什么都不剩时，下面的 has_readable_content 会把整条跳过
    // 打开「朗读筛选 → 朗读表情弹幕」后，两类都按普通弹幕照念。
    let content_src: String = match (config.read_emote, d.emote.is_some()) {
        (true, _) => d.content.clone(),
        (false, true) => return String::new(),
        (false, false) => strip_bracket_emotes(&d.content).0,
    };
    let mut content = clean_for_speech(&content_src);
    if !has_readable_content(&content) {
        return String::new();
    }
    let max = config.max_len as usize;
    if max > 0 && content.chars().count() > max {
        content = content.chars().take(max).collect();
    }

    let mut out = String::new();
    if config.read_role {
        if d.is_admin {
            out.push_str("房管");
        } else if let Some(guard) = d.guard_level.filter(|g| *g >= 1) {
            out.push_str(match guard {
                3 => "舰长",
                2 => "提督",
                _ => "总督",
            });
        }
        if !out.is_empty() {
            out.push(' ');
        }
    }
    if let Some(target) = &d.mention {
        // @提及（B 站单独下发在 info[0][15]，见 `bilibili::mention`）：念成「发送者对被提及者说 正文」。
        // **不看** `read_username`——缺了两边的名字就丢了「对谁说」这层意思。
        // 两边名字同样要剔噪声字符（昵称里的 `_` 会被念成「下划线」）。
        out.push_str(&spoken_name(&d.username));
        out.push_str("对");
        out.push_str(&spoken_name(target));
        out.push_str("说 ");
    } else if config.read_username {
        // 用户名同正文一样要剔噪声字符：昵称里的 `_` 同样会被念成「下划线」
        let name = strip_noise(&d.username);
        let name = name.trim();
        if has_readable_content(name) {
            out.push_str(name);
            out.push_str("说 ");
        }
    }
    out.push_str(&content);
    out.trim().to_string()
}

/// 是否有可读内容：纯标点（`?` `？` `！` `~`）、纯 emoji、纯空白都算「没东西可念」。
/// 汉字与数字在 Unicode 里属于字母/数字，「哈哈哈」「666」都算有内容。
fn has_readable_content(text: &str) -> bool {
    text.chars().any(|ch| ch.is_alphanumeric())
}

/// 组装打赏朗读文案：感谢 + 用户名 + 数量 + 礼物名
///
/// 三种形态各一套句式：礼物「感谢老板A送的5个辣条」、醒目留言「感谢老板A的醒目留言：主播加油」、
/// 上舰「感谢老板A上舰舰长」。礼物名或用户名清洗后为空时降级到不带它的句式，
/// 不出现「感谢送的个」这种断句。
pub(super) fn build_backing_text(b: &Backing) -> String {
    let name = spoken_name(&b.username);
    match b.kind {
        BackingKind::SuperChat => {
            let message = b
                .message
                .as_deref()
                .map(clean_for_speech)
                .map(|m| truncate_chars(&m, MAX_SUPER_CHAT_LEN))
                .unwrap_or_default();
            if message.is_empty() {
                format!("感谢{name}的醒目留言")
            } else {
                format!("感谢{name}的醒目留言：{message}")
            }
        }
        BackingKind::Guard => {
            let tier = clean_for_speech(&b.gift_name);
            if tier.is_empty() {
                format!("感谢{name}上舰")
            } else {
                format!("感谢{name}上舰{tier}")
            }
        }
        // 连击信号不会走到这里（聚合器已忽略），余下的都是普通礼物
        _ => {
            let gift = clean_for_speech(&b.gift_name);
            let num = b.num.max(1);
            if gift.is_empty() {
                format!("感谢{name}送的{num}个礼物")
            } else {
                format!("感谢{name}送的{num}个{gift}")
            }
        }
    }
}

/// 组装欢迎朗读文案（目前只有舰长进场会走这里）
///
/// 句式：「欢迎」+ 舰队档位 + 用户名 + 动作。**档位要念**——不念的话听起来就是
/// 「欢迎某某进入直播间」，主播无法从声音里分出一个总督和一个舰长。
/// 档位与用户名之间留一个空格（与弹幕朗读的身份前缀同一种做法）：
/// 不留的话「欢迎舰长舰长甲」会连读成一串。
/// 用户名清洗后为空时 `spoken_name` 回退「观众」，不会出现缺主语的句子。
pub(super) fn build_welcome_text(w: &Welcome) -> String {
    let tier = match w.guard_level {
        Some(3) => "舰长 ",
        Some(2) => "提督 ",
        Some(1) => "总督 ",
        _ => "",
    };
    format!("欢迎{tier}{}进入直播间", spoken_name(&w.username))
}

/// 朗读用的用户名：昵称里的 `_` 同样会被念成「下划线」，先剔噪声；
/// 剔完为空（纯符号昵称）时回退「观众」，不能让句子缺主语
fn spoken_name(username: &str) -> String {
    let name = strip_noise(username);
    let name = name.trim();
    if has_readable_content(name) {
        name.to_string()
    } else {
        "观众".into()
    }
}

/// 按字符数截断（不是字节数，中文一个字三字节）
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bilibili::event::{Emote, WelcomeKind};

    fn danmaku(content: &str) -> Danmaku {
        Danmaku {
            id: "1".into(),
            username: "小明".into(),
            content: content.into(),
            timestamp: 0,
            color: None,
            medal_level: None,
            medal_name: None,
            medal_room_id: None,
            user_level: None,
            guard_level: None,
            wealth_level: None,
            is_admin: false,
            emote: None,
            mention: None,
        }
    }

    fn welcome(username: &str, guard_level: Option<u32>) -> Welcome {
        Welcome {
            id: "w1".into(),
            kind: WelcomeKind::GuardEnter,
            uid: 10086,
            username: username.into(),
            timestamp: 0,
            guard_level,
        }
    }

    #[test]
    fn 欢迎文案要念出舰队档位() {
        // 不念档位的话听起来就是「欢迎某某进入直播间」，分不出总督和舰长
        assert_eq!(build_welcome_text(&welcome("老板A", Some(3))), "欢迎舰长 老板A进入直播间");
        assert_eq!(build_welcome_text(&welcome("老板A", Some(2))), "欢迎提督 老板A进入直播间");
        assert_eq!(build_welcome_text(&welcome("老板A", Some(1))), "欢迎总督 老板A进入直播间");
    }

    #[test]
    fn 欢迎文案缺档位时不念档位() {
        assert_eq!(build_welcome_text(&welcome("老板A", None)), "欢迎老板A进入直播间");
    }

    #[test]
    fn 欢迎文案用户名不可读时回退观众() {
        assert_eq!(build_welcome_text(&welcome("!!!", Some(3))), "欢迎舰长 观众进入直播间");
    }

    #[test]
    fn build_text_follows_output_switches() {
        let plain = TtsConfig::default();
        let d = danmaku("谢谢礼物");
        assert_eq!(build_text(&d, &plain), "谢谢礼物");

        let with_user = TtsConfig {
            read_username: true,
            ..TtsConfig::default()
        };
        assert_eq!(build_text(&d, &with_user), "小明说 谢谢礼物");
    }

    #[test]
    fn 表情弹幕整条不念() {
        // 正文是表情名（「冲鸭」），念出来没意义；连用户名也不念
        let config = TtsConfig {
            read_username: true,
            read_role: true,
            ..TtsConfig::default()
        };
        let mut d = danmaku("冲鸭");
        d.guard_level = Some(3);
        d.emote = Some(Emote {
            unique: "official_332".into(),
            url: "https://i0.hdslb.com/bfs/emote/x.png".into(),
            width: 20,
            height: 20,
            bulge: false,
        });
        assert_eq!(build_text(&d, &config), "", "表情弹幕应整条跳过");
    }

    #[test]
    fn 开启开关后表情弹幕照常念() {
        // 开关打开就走普通弹幕的路：正文（即表情名）照念
        let config = TtsConfig {
            read_emote: true,
            read_username: true,
            ..TtsConfig::default()
        };
        let mut d = danmaku("冲鸭");
        d.emote = Some(Emote {
            unique: "official_332".into(),
            url: "https://i0.hdslb.com/bfs/live/x.png".into(),
            width: 200,
            height: 60,
            bulge: false,
        });
        assert_eq!(build_text(&d, &config), "小明说 冲鸭");
    }

    #[test]
    fn 无表情字段的老式小表情默认也不念() {
        // 实测：「[花]」这类弹幕的 info[0][13] 是空对象（服务端不给字段），只能按正文形态认
        let config = TtsConfig {
            read_username: true,
            ..TtsConfig::default()
        };
        assert!(danmaku("[花]").emote.is_none(), "样本不带表情字段");
        assert_eq!(build_text(&danmaku("[花]"), &config), "");
        assert_eq!(build_text(&danmaku(" [花] "), &config), "", "首尾空白不影响判定");
        // 弹幕框连点表情会拼成一条（实测 [比心][比心][比心]），多个也得认
        assert_eq!(build_text(&danmaku("[比心][比心][比心]"), &config), "");
        assert_eq!(build_text(&danmaku("[喝彩][喝彩][喝彩][喝彩]"), &config), "");
    }

    #[test]
    fn 表情混着文字时只念文字() {
        let config = TtsConfig::default();
        assert_eq!(build_text(&danmaku("今天真好[花]"), &config), "今天真好");
        assert_eq!(build_text(&danmaku("[花]今天真好"), &config), "今天真好");
        assert_eq!(build_text(&danmaku("今天[花]真好"), &config), "今天真好");
        assert_eq!(build_text(&danmaku("今天真好[花][比心]"), &config), "今天真好");
        // 开关打开：连表情一起念（方括号本身引擎不出声）
        let on = TtsConfig {
            read_username: true,
            read_emote: true,
            ..TtsConfig::default()
        };
        assert_eq!(build_text(&danmaku("今天真好[花]"), &on), "小明说 今天真好[花]");
    }

    #[test]
    fn 开关打开后老式小表情照常念() {
        let config = TtsConfig {
            read_emote: true,
            ..TtsConfig::default()
        };
        // 括号不会被清洗掉（`[` `]` 不在噪声字符表里），引擎实测不念方括号，听起来就是「花」
        assert_eq!(build_text(&danmaku("[花]"), &config), "[花]");
        assert_eq!(build_text(&danmaku("[比心][比心]"), &config), "[比心][比心]");
    }

    #[test]
    fn 不是表情的方括号文字照常念() {
        let config = TtsConfig::default();
        // 未闭合 / 内部还有括号 / 名字过长：都不算表情片段，原样念
        for text in ["[前半", "后半]", "[a[b]c]"] {
            assert_ne!(build_text(&danmaku(text), &config), "", "「{text}」不应被当成表情");
        }
        let long = format!("[{}]", "字".repeat(40));
        assert_ne!(build_text(&danmaku(&long), &config), "", "过长的方括号文本不应被当成表情");
    }

    #[test]
    fn 提及弹幕念成对谁说() {
        // B 站的 @提及不在正文里，而是单独下发在 info[0][15]（见 bilibili::mention）：
        // 实测正文只剩前导空格，名字得从字段取
        let config = TtsConfig {
            read_username: true,
            ..TtsConfig::default()
        };
        let mut d = danmaku(" (⊙o⊙)…");
        d.mention = Some("两米的吉三日".into());
        assert_eq!(build_text(&d, &config), "小明对两米的吉三日说 (⊙o⊙)…");
        // 「朗读用户名」关着也照念两边的名字：这类弹幕缺了名字就丢了「对谁说」
        assert_eq!(build_text(&d, &TtsConfig::default()), "小明对两米的吉三日说 (⊙o⊙)…");
        // 被提及的名字同样过噪声剔除
        let mut noisy = danmaku("谢谢");
        noisy.mention = Some("雨月云__".into());
        assert_eq!(build_text(&noisy, &TtsConfig::default()), "小明对雨月云说 谢谢");
    }

    #[test]
    fn 只提及没说话的不念() {
        // 服务端把 @名字 从正文里剥掉了，只 @ 人不说话时正文为空：
        // 念「小明对两米的吉三日说」是句半截话，整条跳过
        let mut d = danmaku(" ");
        d.mention = Some("两米的吉三日".into());
        assert_eq!(build_text(&d, &TtsConfig::default()), "");
    }

    #[test]
    fn 正文里手打的_at_不算提及() {
        // 没有 reply_uname 字段就按普通弹幕照念；`@` 本身是会被念成「at」的噪声字符，清洗时剔掉
        assert_eq!(
            build_text(&danmaku("谢谢@某人 的礼物"), &TtsConfig::default()),
            "谢谢某人 的礼物"
        );
    }

    #[test]
    fn build_text_prefixes_role_and_truncates() {
        let config = TtsConfig {
            read_role: true,
            max_len: 3,
            ..TtsConfig::default()
        };
        let mut d = danmaku("谢谢谢谢礼物");
        d.guard_level = Some(3);
        // 折叠后正文是「谢谢谢礼物」，截到 3 字；前缀「舰长」不计入上限
        assert_eq!(build_text(&d, &config), "舰长 谢谢谢");
    }

    #[test]
    fn max_len_counts_content_only() {
        // 长昵称 + 身份前缀不能挤掉正文额度
        let config = TtsConfig {
            read_role: true,
            read_username: true,
            max_len: 5,
            ..TtsConfig::default()
        };
        let mut d = danmaku("今天天气真不错呢");
        d.username = "森罗万象之主宰".into();
        d.guard_level = Some(3);
        assert_eq!(
            build_text(&d, &config),
            "舰长 森罗万象之主宰说 今天天气真"
        );
    }

    #[test]
    fn cleans_underscores_in_username_too() {
        // 昵称 `雨月云__` 里的下划线同样会被念成「下划线」，昵称也走噪声剔除
        let config = TtsConfig {
            read_username: true,
            ..TtsConfig::default()
        };
        let mut d = danmaku("好好好");
        d.username = "雨月云__".into();
        assert_eq!(build_text(&d, &config), "雨月云说 好好好");
    }

    #[test]
    fn skips_username_that_is_all_noise() {
        let config = TtsConfig {
            read_username: true,
            ..TtsConfig::default()
        };
        let mut d = danmaku("好好好");
        d.username = "_＿_".into();
        assert_eq!(build_text(&d, &config), "好好好");
    }

    #[test]
    fn user_name_keeps_repeats() {
        // 昵称里的重复字符不折叠（那是用户的名字，不是拖长音）
        assert_eq!(strip_noise("哈哈哈哈哈__"), "哈哈哈哈哈");
    }

    #[test]
    fn empty_content_after_cleaning_is_not_spoken_at_all() {
        // 正文没东西可念时，不该只剩下「舰长 小明说」这种空壳
        let config = TtsConfig {
            read_role: true,
            read_username: true,
            ..TtsConfig::default()
        };
        let mut d = danmaku("😄🎉");
        d.guard_level = Some(3);
        assert_eq!(build_text(&d, &config), "");
        assert_eq!(build_text(&danmaku("__"), &config), "");
        assert_eq!(build_text(&danmaku("https://b23.tv/abc"), &config), "");
        assert_eq!(build_text(&danmaku("   "), &config), "");
    }

    #[test]
    fn punctuation_only_content_is_not_spoken_at_all() {
        // 「？」「！」这类符号引擎不出声，只剩前缀就会念成「小明说」+ 空白
        let config = TtsConfig {
            read_role: true,
            read_username: true,
            ..TtsConfig::default()
        };
        let mut d = danmaku("?");
        d.guard_level = Some(3);
        assert_eq!(build_text(&d, &config), "");
        assert_eq!(build_text(&danmaku("？？？"), &config), "");
        assert_eq!(build_text(&danmaku("！ 。 ~"), &config), "");
        // 标点里夹了字就得念，只把无内容的标点留在后面无妨
        assert_eq!(build_text(&danmaku("?好"), &config), "小明说 ?好");
    }

    #[test]
    fn punctuation_only_username_is_skipped() {
        let config = TtsConfig {
            read_username: true,
            ..TtsConfig::default()
        };
        let mut d = danmaku("谢谢你");
        d.username = "？？".into();
        assert_eq!(build_text(&d, &config), "谢谢你");
    }

    #[test]
    fn sensitive_word_drops_whole_danmaku_including_prefix() {
        let config = TtsConfig {
            read_role: true,
            read_username: true,
            filter: DanmakuFilter {
                enable_sensitive: true,
                sensitive_words: vec!["哈哈".into()],
                ..DanmakuFilter::default()
            },
            ..TtsConfig::default()
        };
        let mut d = danmaku("哈哈哈哈");
        d.guard_level = Some(3);
        // 筛选在组装文案之前：命中敏感词就整条丢掉，不会念出「舰长 小明说」
        assert!(!matches_filter(&d, &config.filter));
    }

    fn backing(kind: BackingKind, username: &str, gift_name: &str, num: u32) -> Backing {
        Backing {
            id: "1".into(),
            kind,
            uid: 1,
            username: username.into(),
            gift_name: gift_name.into(),
            gift_id: 100,
            num,
            amount_fen: 100,
            timestamp: 0,
            message: None,
            guard_level: None,
            wealth_level: None,
        }
    }

    #[test]
    fn gift_text_reads_username_and_count() {
        let five = backing(BackingKind::Gift, "老板A", "辣条", 5);
        assert_eq!(build_backing_text(&five), "感谢老板A送的5个辣条");
        let one = backing(BackingKind::Gift, "老板A", "辣条", 1);
        assert_eq!(build_backing_text(&one), "感谢老板A送的1个辣条");
    }

    #[test]
    fn super_chat_text_speaks_message() {
        let mut b = backing(BackingKind::SuperChat, "老板A", "醒目留言", 1);
        b.message = Some("主播加油😄".into());
        assert_eq!(build_backing_text(&b), "感谢老板A的醒目留言：主播加油");
        // 留言缺失/清洗后为空时不能出现「的醒目留言：」这种空尾巴
        b.message = None;
        assert_eq!(build_backing_text(&b), "感谢老板A的醒目留言");
        b.message = Some("😄🎉".into());
        assert_eq!(build_backing_text(&b), "感谢老板A的醒目留言");
    }

    #[test]
    fn long_super_chat_message_is_truncated() {
        let mut b = backing(BackingKind::SuperChat, "老板A", "醒目留言", 1);
        b.message = Some("主播加油".repeat(20));
        let text = build_backing_text(&b);
        let body = text.split('：').nth(1).expect("应保留留言正文");
        assert_eq!(body.chars().count(), MAX_SUPER_CHAT_LEN);
    }

    #[test]
    fn guard_text_names_the_tier() {
        let b = backing(BackingKind::Guard, "老板A", "舰长", 1);
        assert_eq!(build_backing_text(&b), "感谢老板A上舰舰长");
    }

    #[test]
    fn unreadable_name_or_gift_falls_back() {
        // 纯符号昵称 → 回退「观众」
        let noise_name = backing(BackingKind::Gift, "_＿_", "辣条", 3);
        assert_eq!(build_backing_text(&noise_name), "感谢观众送的3个辣条");
        // 礼物名清洗后为空 → 不出现「送的3个」空尾巴
        let noise_gift = backing(BackingKind::Gift, "老板A", "***", 3);
        assert_eq!(build_backing_text(&noise_gift), "感谢老板A送的3个礼物");
    }

    #[test]
    fn filter_matches_chosen_identities() {
        let admins_only = DanmakuFilter {
            enable_guard_admin: true,
            ..DanmakuFilter::default()
        };
        let mut d = danmaku("你好");
        assert!(!matches_filter(&d, &admins_only));
        d.is_admin = true;
        assert!(matches_filter(&d, &admins_only));
        // 敏感词屏蔽独立叠加，命中时房管也不豁免
        let blocked = DanmakuFilter {
            enable_guard_admin: true,
            enable_sensitive: true,
            sensitive_words: vec!["广告".into()],
            ..DanmakuFilter::default()
        };
        assert!(!matches_filter(&danmaku("加群 广告"), &blocked));
    }
}
