//! DANMU_MSG（普通弹幕）解析
//!
//! 从 `parser.rs` 拆出（那边代码行数超过 300）。info 数组下标映射见函数注释；
//! 表情包（`info[0][13]`）的解析在 `bilibili::emote`。

use super::fallback_now;
use crate::bilibili::emote::{parse_emote, strip_bracket_emotes};
use crate::bilibili::event::Danmaku;

/// 解析 DANMU_MSG 命令 → Danmaku
///
/// info 数组结构（解析失败时容错返回 None，不影响主流程）：
/// ```text
/// info[0] = [0, mode, fontsize, color_dec, ts, random_id, ...]
/// info[1] = 弹幕内容（字符串或 [字符串]）
/// info[2] = [uid, uname, isadmin, ...]
/// ```
pub(super) fn parse_danmaku(v: &serde_json::Value) -> Option<Danmaku> {
    let info = v.get("info")?.as_array()?;

    // 内容：info[1] 可能直接是字符串，也可能是单元素数组
    let content = match info.get(1) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Array(a)) => a.first()?.as_str()?.to_string(),
        _ => return None,
    };
    if content.trim().is_empty() {
        return None;
    }

    // 用户：info[2][0]=uid, info[2][1]=uname
    let user_info = info.get(2)?.as_array()?;
    let uid = user_info
        .first()
        .and_then(|u| u.as_i64())
        .unwrap_or(0);
    let username = user_info.get(1).and_then(|u| u.as_str()).unwrap_or("").to_string();

    // 元信息：info[0][3]=颜色, info[0][4]=时间戳(毫秒), info[0][5]=随机id
    let head = info.first().and_then(|h| h.as_array());
    let timestamp = head
        .and_then(|h| h.get(4))
        .and_then(|t| t.as_i64())
        // B 站新版毫秒时间戳（13 位）统一转秒
        .map(|t| if t > 10_000_000_000 { t / 1000 } else { t })
        .unwrap_or_else(fallback_now);
    let color = head
        .and_then(|h| h.get(3))
        .and_then(|c| c.as_u64())
        .map(|c| format!("#{:06X}", c & 0xFFFFFF));
    let random = head.and_then(|h| h.get(5)).and_then(|r| r.as_u64()).unwrap_or(0);

    let id = format!("{}-{}-{}", timestamp, uid, random);

    // ---- 用户身份（字段位置参考 blivedm DanmakuMessage.from_command） ----
    // 勋章：info[3] = [等级, 勋章名, 主播名, 勋章房间id, 颜色, ...]，空数组=未佩戴
    let (medal_level, medal_name, medal_room_id) = match info.get(3).and_then(|m| m.as_array()) {
        Some(m) if !m.is_empty() => (
            m.first().and_then(|v| v.as_u64()).map(|v| v as u32),
            m.get(1).and_then(|v| v.as_str()).map(String::from),
            m.get(3).and_then(|v| v.as_u64()).map(|v| v as u32),
        ),
        _ => (None, None, None),
    };
    // 用户等级：info[4] = [等级, ?, 等级颜色, 排名(如 ">50000"), ...]
    let user_level = info
        .get(4)
        .and_then(|l| l.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    // 舰队等级：info[7]（3=舰长 2=提督 1=总督）；房管：info[2][2]
    let guard_level = info.get(7).and_then(|v| v.as_u64()).map(|v| v as u32);
    // 荣耀等级（全站财富等级）：info[16][0]
    let wealth_level = info
        .get(16)
        .and_then(|w| w.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_u64())
        .map(|v| v as u32);
    let is_admin = user_info.get(2).and_then(|v| v.as_u64()).unwrap_or(0) > 0;
    // 表情包：info[0][13]。无表情时该位是字符串 "{}"（不是空对象），故解析必须容错
    let emote = head.and_then(|h| h.get(13)).and_then(parse_emote);

    // 逐条弹幕的识别日志走 debug：日志级别是 Info，平时不落盘、不刷屏；
    // 排查「表情弹幕没被识别/还在被念」时把 lib.rs 的 level 临时改成 Debug 即可看到
    if let Some(e) = &emote {
        log::debug!(
            "[emote] 识别表情弹幕「{content}」unique={} {}x{} bulge={}",
            e.unique,
            e.width,
            e.height,
            e.bulge
        );
    } else {
        let (stripped, had_emote) = strip_bracket_emotes(&content);
        if had_emote {
            log::debug!("[emote] 含表情弹幕「{content}」无表情字段，朗读只念「{stripped}」");
        }
    }

    Some(Danmaku {
        id,
        username,
        content,
        timestamp,
        color,
        medal_level,
        medal_name,
        medal_room_id,
        user_level,
        guard_level,
        wealth_level,
        is_admin,
        emote,
    })
}
