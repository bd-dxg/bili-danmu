//! 打赏类命令解析：SEND_GIFT / SEND_GIFT_V2 / COMBO_SEND / SUPER_CHAT_MESSAGE / GUARD_BUY
//!
//! 从 `parser.rs` 拆出（那边代码行数超过 300）。金额一律折算成整数「分」
//! （`amount_fen`，金瓜子 1000 = 1 元），各函数注释里写明原始单位；
//! 免费礼物（银瓜子 / 零价）直接返回 None。
//!
//! 连击合并与金额门槛在 `crate::gift`，礼物朗读在 `crate::tts::gift`，这里只负责解包。

use super::{coin_to_fen, get_i64, get_str, get_u32, normalize_timestamp};
use crate::bilibili::event::{Backing, BackingKind};
use crate::bilibili::protobuf;

/// 解析 SEND_GIFT → Backing
///
/// 免费礼物（银瓜子 / 单价为 0）返回 None：没有人民币价值，进礼物列表只会刷屏，
/// 也无法参与金额门槛计算。
///
/// data 字段（含具体金额口径）：
/// ```text
/// giftName / num / uname / uid / timestamp
/// price      = 礼物单价（金瓜子，送盲盒时是爆出礼物的单价）
/// total_coin = 实付总价（金瓜子）= 折后单价 x num，优先用它算金额
/// coin_type  = 'gold' 付费 / 'silver' 免费
/// rnd        = 服务端去重随机数（时间戳+去重 ID 或 UUID）
/// ```
pub(super) fn parse_gift(v: &serde_json::Value) -> Option<Backing> {
    let data = v.get("data")?;

    let coin_type = data.get("coin_type").and_then(|c| c.as_str()).unwrap_or("");
    let price = get_i64(data, "price").unwrap_or(0);
    let total_coin = get_i64(data, "total_coin").unwrap_or(0);
    if coin_type == "silver" || (price <= 0 && total_coin <= 0) {
        return None;
    }

    let uid = get_i64(data, "uid").unwrap_or(0);
    let gift_id = get_i64(data, "giftId").unwrap_or(0);
    let num = get_u32(data, "num").unwrap_or(1).max(1);
    // 实付总价缺失（部分礼物不下发 total_coin）时退回 单价 x 数量
    let coin = if total_coin > 0 {
        total_coin
    } else {
        price * num as i64
    };
    let timestamp = normalize_timestamp(get_i64(data, "timestamp").unwrap_or(0));
    let rnd = get_str(data, "rnd").unwrap_or_default();

    Some(Backing {
        id: if rnd.is_empty() {
            format!("gift-{timestamp}-{uid}-{gift_id}")
        } else {
            format!("gift-{rnd}")
        },
        kind: BackingKind::Gift,
        uid,
        username: get_str(data, "uname").unwrap_or_default(),
        gift_name: get_str(data, "giftName").unwrap_or_default(),
        gift_id,
        num,
        amount_fen: coin_to_fen(coin),
        timestamp,
        message: None,
        guard_level: get_u32(data, "guard_level"),
        wealth_level: get_u32(data, "wealth_level"),
    })
}

/// 解析 SEND_GIFT_V2（新版礼物广播）→ 若干 Backing
///
/// 载荷是 base64 编码的 protobuf（字段号见 `protobuf` 模块文档），
/// 一条消息可携带多个礼物档位；用户字段（uid / uname / 舰队）在顶层，
/// 每个礼物档位自己带 gift_id / 数量 / 价格。
/// 免费礼物（银瓜子 / 零价）与 SEND_GIFT 同一口径，直接丢弃。
pub(super) fn parse_gift_v2(v: &serde_json::Value) -> Vec<Backing> {
    let Some(raw) = v
        .get("data")
        .and_then(|d| d.get("pb"))
        .and_then(|p| p.as_str())
        .and_then(protobuf::decode_base64)
    else {
        return Vec::new();
    };
    let Ok(top) = protobuf::decode(&raw) else {
        return Vec::new();
    };

    let uid = protobuf::int(&top, 1).unwrap_or(0) as i64;
    let username = protobuf::string(&top, 2).unwrap_or_default();
    let guard_level = protobuf::int(&top, 5).map(|g| g as u32);

    let mut out = Vec::new();
    for item in protobuf::bytes_all(&top, 10) {
        let Ok(f) = protobuf::decode(item) else {
            continue;
        };
        let coin_type = protobuf::string(&f, 8).unwrap_or_default();
        let price = protobuf::int(&f, 5).unwrap_or(0) as i64;
        let total_coin = protobuf::int(&f, 7).unwrap_or(0) as i64;
        if coin_type == "silver" || (price <= 0 && total_coin <= 0) {
            continue;
        }

        let gift_id = protobuf::int(&f, 1).unwrap_or(0) as i64;
        let num = protobuf::int(&f, 3).unwrap_or(1).max(1) as u32;
        let coin = if total_coin > 0 {
            total_coin
        } else {
            price * num as i64
        };
        let timestamp = normalize_timestamp(protobuf::int(&f, 10).unwrap_or(0) as i64);
        let rnd = protobuf::string(&f, 12).unwrap_or_default();

        out.push(Backing {
            id: if rnd.is_empty() {
                format!("gift-{timestamp}-{uid}-{gift_id}")
            } else {
                format!("gift-{rnd}")
            },
            kind: BackingKind::Gift,
            uid,
            username: username.clone(),
            gift_name: protobuf::string(&f, 2).unwrap_or_default(),
            gift_id,
            num,
            amount_fen: coin_to_fen(coin),
            timestamp,
            message: None,
            guard_level,
            // 新版广播里没有荣耀等级，礼物行的 LV 徽章会缺一档
            wealth_level: None,
        });
    }
    out
}

/// 解析 COMBO_SEND（连击）→ Backing
///
/// 只当「连击仍在继续」的信号用：数量与金额恒为 0，一律以 SEND_GIFT 为准。
/// 连击期间两路事件同时下发，若 COMBO_SEND 也计入金额就会重复计数；
/// 其是否携带价格字段未经实测（可用 `scripts/ws-probe.ps1` 验证后再决定是否改口径）。
/// 字段名在不同版本里有 snake_case / camelCase 两种写法，这里都做兼容。
pub(super) fn parse_combo(v: &serde_json::Value) -> Option<Backing> {
    let data = v.get("data")?;
    let uid = get_i64(data, "uid").unwrap_or(0);
    let gift_id = get_i64(data, "gift_id")
        .or_else(|| get_i64(data, "giftId"))
        .unwrap_or(0);
    let timestamp = normalize_timestamp(get_i64(data, "timestamp").unwrap_or(0));

    Some(Backing {
        id: format!("combo-{timestamp}-{uid}-{gift_id}"),
        kind: BackingKind::Combo,
        uid,
        username: get_str(data, "uname")
            .or_else(|| get_str(data, "username"))
            .unwrap_or_default(),
        gift_name: get_str(data, "gift_name")
            .or_else(|| get_str(data, "giftName"))
            .unwrap_or_default(),
        gift_id,
        num: 0,
        amount_fen: 0,
        timestamp,
        message: None,
        guard_level: get_u32(data, "guard_level"),
        wealth_level: get_u32(data, "wealth_level"),
    })
}

/// 解析 SUPER_CHAT_MESSAGE → Backing
///
/// 注意 price 的单位是人民币元（与礼物的金瓜子不同），金额口径在此单独折算。
pub(super) fn parse_super_chat(v: &serde_json::Value) -> Option<Backing> {
    let data = v.get("data")?;
    let user = data.get("user_info");
    let uid = get_i64(data, "uid").unwrap_or(0);
    let timestamp = normalize_timestamp(get_i64(data, "start_time").unwrap_or(0));
    let price = get_i64(data, "price").unwrap_or(0).max(0);

    Some(Backing {
        id: match get_i64(data, "id") {
            Some(id) if id != 0 => format!("sc-{id}"),
            _ => format!("sc-{timestamp}-{uid}"),
        },
        kind: BackingKind::SuperChat,
        uid,
        username: user
            .and_then(|u| get_str(u, "uname"))
            .unwrap_or_default(),
        gift_name: "醒目留言".to_string(),
        gift_id: get_i64(data, "gift_id").unwrap_or(0),
        num: 1,
        amount_fen: price as u64 * 100,
        timestamp,
        message: get_str(data, "message"),
        guard_level: get_u32(data, "guard_level"),
        wealth_level: user.and_then(|u| get_u32(u, "wealth_level")),
    })
}

/// 解析 GUARD_BUY（上舰）→ Backing
///
/// price 是单价金瓜子数，与 num 相乘后折算；gift_name 在新版协议里可能不下发，
/// 缺失时按舰队等级补名称。
pub(super) fn parse_guard_buy(v: &serde_json::Value) -> Option<Backing> {
    let data = v.get("data")?;
    let uid = get_i64(data, "uid").unwrap_or(0);
    let guard_level = get_u32(data, "guard_level").unwrap_or(0);
    let num = get_u32(data, "num").unwrap_or(1).max(1);
    let price = get_i64(data, "price").unwrap_or(0).max(0);
    let timestamp = normalize_timestamp(get_i64(data, "start_time").unwrap_or(0));

    Some(Backing {
        id: format!("guard-{timestamp}-{uid}-{guard_level}"),
        kind: BackingKind::Guard,
        uid,
        username: get_str(data, "username")
            .or_else(|| get_str(data, "uname"))
            .unwrap_or_default(),
        gift_name: get_str(data, "gift_name")
            .unwrap_or_else(|| guard_name(guard_level).to_string()),
        gift_id: get_i64(data, "gift_id").unwrap_or(0),
        num,
        amount_fen: coin_to_fen(price * num as i64),
        timestamp,
        message: None,
        guard_level: Some(guard_level),
        wealth_level: get_u32(data, "wealth_level"),
    })
}

/// 舰队等级 → 档位名（1 总督 / 2 提督 / 3 舰长）
fn guard_name(level: u32) -> &'static str {
    match level {
        1 => "总督",
        2 => "提督",
        _ => "舰长",
    }
}
