//! 欢迎类命令解析：INTERACT_WORD_V2（进房 / 关注 / 分享）/ LIKE_INFO_V3_CLICK（点赞）/ ENTRY_EFFECT（舰长进场）
//!
//! 从 `parser.rs` 拆出（那边代码行数超过 300）。去重与限速在 `crate::welcome`
//! （上层消费者），这里只负责解包；字段号均为实测得出，见各函数注释。

use super::{fallback_now, normalize_timestamp};
use crate::bilibili::event::{Welcome, WelcomeKind};
use crate::bilibili::protobuf;

/// 取 `uinfo.guard.level`，0（无大航海）归一成 None
fn guard_level_of(d: &serde_json::Value) -> Option<u32> {
    d.pointer("/uinfo/guard/level")
        .and_then(|x| x.as_u64())
        .map(|l| l as u32)
        .filter(|l| *l > 0)
}

/// 解析 INTERACT_WORD_V2（进房 / 关注 / 分享）→ Welcome
///
/// 载荷是 base64 的 protobuf（字段号实测得出）：1=uid、2=用户名、5=msg_type、
/// 6=房间号、7=时间戳(秒)。msg_type：1 进入 / 2 关注 / 3 分享
/// （2 已实测到，3 未抓到、按 B 站惯例），其余取值一律丢——宁可漏也不要张冠李戴。
///
/// 注意：这条事件里**没有** guard_level（60 个样本做了全字段扫描确认，
/// 22.4.1 那个 1..33 的值是等级不是舰队），所以「只念舰长以上」不能靠进房事件，
/// 只能靠 ENTRY_EFFECT。
pub(super) fn parse_interact_word_v2(v: &serde_json::Value) -> Option<Welcome> {
    let raw = v
        .get("data")?
        .get("pb")?
        .as_str()
        .and_then(protobuf::decode_base64)?;
    let f = protobuf::decode(&raw).ok()?;
    let msg_type = protobuf::int(&f, 5)?;
    let kind = match msg_type {
        1 => WelcomeKind::Enter,
        2 => WelcomeKind::Follow,
        3 => WelcomeKind::Share,
        _ => return None,
    };
    let uid = protobuf::int(&f, 1).unwrap_or(0) as i64;
    let timestamp = normalize_timestamp(protobuf::int(&f, 7).unwrap_or(0) as i64);
    Some(Welcome {
        id: format!("iw-{msg_type}-{uid}-{timestamp}"),
        kind,
        uid,
        username: protobuf::string(&f, 2).unwrap_or_default(),
        timestamp,
        guard_level: None,
    })
}

/// 解析 LIKE_INFO_V3_CLICK（单次点赞）→ Welcome
///
/// 纯 JSON，不需要 protobuf。`LIKE_INFO_V3_UPDATE` 故意不解析：
/// 它只有累计点赞总数（`click_count`），不是「事件」，单条点赞用这条就够。
pub(super) fn parse_like_click(v: &serde_json::Value) -> Option<Welcome> {
    let d = v.get("data")?;
    let uid = d.get("uid").and_then(|x| x.as_i64()).unwrap_or(0);
    // 载荷里没有时间戳字段，用系统时间（同人同一秒的连点会因此合并成一条）
    let timestamp = fallback_now();
    Some(Welcome {
        id: format!("like-{uid}-{timestamp}"),
        kind: WelcomeKind::Like,
        uid,
        username: d
            .get("uname")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string(),
        timestamp,
        guard_level: guard_level_of(d),
    })
}

/// 解析 ENTRY_EFFECT（舰长 / 提督 / 总督进场特效）→ Welcome
///
/// 这条**不是**只给大航海用的：荣耀等级高的普通观众进场也会触发（实测有
/// `guard.level=0` + `wealth.level=15` 的样本），所以必须按 `guard.level>0` 过滤，
/// 否则「上舰欢迎」会变成「高等级观众欢迎」。
///
/// 载荷里没有可直接用的时间戳（`trigger_time` 是 100ns 单位，且与常规时间戳不同源），
/// 统一用系统时间。
pub(super) fn parse_entry_effect(v: &serde_json::Value) -> Option<Welcome> {
    let d = v.get("data")?;
    let guard_level = guard_level_of(d)?;
    let uid = d.get("uid").and_then(|x| x.as_i64()).unwrap_or(0);
    let timestamp = fallback_now();
    Some(Welcome {
        id: format!("guard-{uid}-{timestamp}"),
        kind: WelcomeKind::GuardEnter,
        uid,
        username: d
            .pointer("/uinfo/base/name")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string(),
        timestamp,
        guard_level: Some(guard_level),
    })
}
