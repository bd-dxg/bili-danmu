//! B 站弹幕协议：消息展开（解压）与 JSON 命令解析
//!
//! 服务器下发链：网络帧(Op5, proto 0/2/3)
//!   → proto 0: body 即 JSON（可能为多个命令的数组）
//!   → proto 2/3: body 是 Zlib/Brotli 压缩的嵌套包流，解压后递归拆包

use crate::bilibili::event::{Backing, BackingKind, BilibiliEvent, Danmaku};
use crate::bilibili::protobuf;
use crate::bilibili::protocol::{decode_stream, Packet, PROTO_BROTLI, PROTO_JSON, PROTO_ZLIB};
use brotli_decompressor::Decompressor;
use flate2::read::ZlibDecoder;
use std::io::Read;

/// Zlib 解压
fn inflate_zlib(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|e| format!("zlib 解压失败: {e}"))?;
    Ok(out)
}

/// Brotli 解压
fn inflate_brotli(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    Decompressor::new(data, 4096)
        .read_to_end(&mut out)
        .map_err(|e| format!("brotli 解压失败: {e}"))?;
    Ok(out)
}

/// 展开一个包：Op=5 时按协议版本解压/解包，返回若干可直接当作 JSON 解析的 body
pub fn expand_packet(p: &Packet) -> Result<Vec<Vec<u8>>, String> {
    if p.op != crate::bilibili::protocol::OP_MESSAGE {
        return Ok(Vec::new()); // 心跳/认证响应等不在此处理
    }
    match p.proto {
        PROTO_JSON => Ok(vec![p.body.clone()]),
        PROTO_ZLIB | PROTO_BROTLI => {
            let raw = if p.proto == PROTO_ZLIB {
                inflate_zlib(&p.body)?
            } else {
                inflate_brotli(&p.body)?
            };
            // 解压结果是一段嵌套的包流（可能多包），递归展开
            let mut out = Vec::new();
            for inner in decode_stream(&raw)? {
                out.extend(expand_packet(&inner)?);
            }
            Ok(out)
        }
        _ => Ok(Vec::new()),
    }
}

/// 把单个字节数组（可能是单个 JSON 对象，也可能是一次多条命令的数组）解析成事件
pub fn parse_payload(bytes: &[u8]) -> Result<Vec<BilibiliEvent>, String> {
    let text = String::from_utf8_lossy(bytes);
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("JSON 解析失败: {e}"))?;

    let items: Vec<&serde_json::Value> = match &value {
        serde_json::Value::Array(arr) => arr.iter().collect(),
        _ => vec![&value],
    };

    let mut events = Vec::new();
    for item in items {
        let cmd = item.get("cmd").and_then(|c| c.as_str()).unwrap_or("");
        match cmd {
            "DANMU_MSG" => {
                if let Some(d) = parse_danmaku(item) {
                    events.push(BilibiliEvent::Danmaku(d));
                }
            }
            "SEND_GIFT" => {
                if let Some(b) = parse_gift(item) {
                    events.push(BilibiliEvent::Backing(b));
                }
            }
            // 新版礼物广播：载荷是 protobuf，一条消息可携带多个礼物档位
            "SEND_GIFT_V2" => {
                events.extend(parse_gift_v2(item).into_iter().map(BilibiliEvent::Backing));
            }
            "COMBO_SEND" => {
                if let Some(b) = parse_combo(item) {
                    events.push(BilibiliEvent::Backing(b));
                }
            }
            "SUPER_CHAT_MESSAGE" => {
                if let Some(b) = parse_super_chat(item) {
                    events.push(BilibiliEvent::Backing(b));
                }
            }
            "GUARD_BUY" => {
                if let Some(b) = parse_guard_buy(item) {
                    events.push(BilibiliEvent::Backing(b));
                }
            }
            // 其它命令（进场/通知等）暂无消费者，直接忽略——逐条构造事件
            // 并打印日志会在大房间高频事件流下造成日志洪泛与无谓分配
            _ => {}
        }
    }
    Ok(events)
}

/// 解析 DANMU_MSG 命令 → Danmaku
///
/// info 数组结构（解析失败时容错返回 None，不影响主流程）：
/// ```text
/// info[0] = [0, mode, fontsize, color_dec, ts, random_id, ...]
/// info[1] = 弹幕内容（字符串或 [字符串]）
/// info[2] = [uid, uname, isadmin, ...]
/// ```
fn parse_danmaku(v: &serde_json::Value) -> Option<Danmaku> {
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
    })
}

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
/// medal_info = { medal_level, medal_name, medal_room_id / anchor_roomid }
/// ```
fn parse_gift(v: &serde_json::Value) -> Option<Backing> {
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
    let (medal_level, medal_name, medal_room_id) = parse_medal(data.get("medal_info"));

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
        medal_level,
        medal_name,
        medal_room_id,
        guard_level: get_u32(data, "guard_level"),
        wealth_level: get_u32(data, "wealth_level"),
    })
}

/// 解析 SEND_GIFT_V2（新版礼物广播）→ 若干 Backing
///
/// 载荷是 base64 编码的 protobuf（字段号见 `protobuf` 模块文档），
/// 一条消息可携带多个礼物档位；用户字段（uid / uname / 舰队 / 勋章）在顶层，
/// 每个礼物档位自己带 gift_id / 数量 / 价格。
/// 免费礼物（银瓜子 / 零价）与 SEND_GIFT 同一口径，直接丢弃。
fn parse_gift_v2(v: &serde_json::Value) -> Vec<Backing> {
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
    // 勋章：嵌套消息（anchor_roomid / medal_level / medal_name）
    let medal = protobuf::bytes(&top, 8).and_then(|b| protobuf::decode(b).ok());
    let (medal_level, medal_name, medal_room_id) = match medal {
        Some(m) => (
            protobuf::int(&m, 5).map(|l| l as u32).filter(|l| *l > 0),
            protobuf::string(&m, 6),
            protobuf::int(&m, 4).map(|r| r as u32),
        ),
        None => (None, None, None),
    };

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
            medal_level,
            medal_name: medal_name.clone(),
            medal_room_id,
            guard_level,
            // 新版广播里没有荣耀等级，礼物行的 LV 徽章会缺一档
            wealth_level: None,
        });
    }
    out
}

/// 时间戳归一化：13 位毫秒转秒，0 或缺失时用系统时间。
/// 各命令一律走这里，避免某条路径忘了转毫秒写出错误时间。
fn normalize_timestamp(t: i64) -> i64 {
    if t <= 0 {
        return fallback_now();
    }
    if t > 10_000_000_000 {
        t / 1000
    } else {
        t
    }
}

/// 解析 COMBO_SEND（连击）→ Backing
///
/// 只当「连击仍在继续」的信号用：数量与金额恒为 0，一律以 SEND_GIFT 为准。
/// 连击期间两路事件同时下发，若 COMBO_SEND 也计入金额就会重复计数；
/// 其是否携带价格字段未经实测（可用 `scripts/ws-probe.ps1` 验证后再决定是否改口径）。
/// 字段名在不同版本里有 snake_case / camelCase 两种写法，这里都做兼容。
fn parse_combo(v: &serde_json::Value) -> Option<Backing> {
    let data = v.get("data")?;
    let uid = get_i64(data, "uid").unwrap_or(0);
    let gift_id = get_i64(data, "gift_id")
        .or_else(|| get_i64(data, "giftId"))
        .unwrap_or(0);
    let timestamp = normalize_timestamp(get_i64(data, "timestamp").unwrap_or(0));
    let (medal_level, medal_name, medal_room_id) = parse_medal(data.get("medal_info"));

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
        medal_level,
        medal_name,
        medal_room_id,
        guard_level: get_u32(data, "guard_level"),
        wealth_level: get_u32(data, "wealth_level"),
    })
}

/// 解析 SUPER_CHAT_MESSAGE → Backing
///
/// 注意 price 的单位是人民币元（与礼物的金瓜子不同），金额口径在此单独折算。
fn parse_super_chat(v: &serde_json::Value) -> Option<Backing> {
    let data = v.get("data")?;
    let user = data.get("user_info");
    let uid = get_i64(data, "uid").unwrap_or(0);
    let timestamp = normalize_timestamp(get_i64(data, "start_time").unwrap_or(0));
    let price = get_i64(data, "price").unwrap_or(0).max(0);
    let (medal_level, medal_name, medal_room_id) = parse_medal(data.get("medal_info"));

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
        medal_level,
        medal_name,
        medal_room_id,
        guard_level: get_u32(data, "guard_level"),
        wealth_level: user.and_then(|u| get_u32(u, "wealth_level")),
    })
}

/// 解析 GUARD_BUY（上舰）→ Backing
///
/// price 是单价金瓜子数，与 num 相乘后折算；gift_name 在新版协议里可能不下发，
/// 缺失时按舰队等级补名称。
fn parse_guard_buy(v: &serde_json::Value) -> Option<Backing> {
    let data = v.get("data")?;
    let uid = get_i64(data, "uid").unwrap_or(0);
    let guard_level = get_u32(data, "guard_level").unwrap_or(0);
    let num = get_u32(data, "num").unwrap_or(1).max(1);
    let price = get_i64(data, "price").unwrap_or(0).max(0);
    let timestamp = normalize_timestamp(get_i64(data, "start_time").unwrap_or(0));
    let (medal_level, medal_name, medal_room_id) = parse_medal(data.get("medal_info"));

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
        medal_level,
        medal_name,
        medal_room_id,
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

/// 金瓜子 → 分（1000 金瓜子 = 1 元 = 100 分）
fn coin_to_fen(coin: i64) -> u64 {
    (coin.max(0) as u64) / 10
}

/// 解析勋章信息 → (等级, 名称, 所属房间 ID)；未佩戴或字段缺失时全为 None
fn parse_medal(m: Option<&serde_json::Value>) -> (Option<u32>, Option<String>, Option<u32>) {
    let Some(m) = m.filter(|m| !m.is_null()) else {
        return (None, None, None);
    };
    (
        get_u32(m, "medal_level").filter(|l| *l > 0),
        get_str(m, "medal_name"),
        // 房间 ID 字段名有 medal_room_id / anchor_roomid 两种
        get_u32(m, "medal_room_id").or_else(|| get_u32(m, "anchor_roomid")),
    )
}

/// 读取字符串字段（缺失或类型不符返回 None）
fn get_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(String::from)
}

/// 读取 u32 字段（缺失或类型不符返回 None）
fn get_u32(v: &serde_json::Value, key: &str) -> Option<u32> {
    v.get(key).and_then(|x| x.as_u64()).map(|n| n as u32)
}

/// 读取 i64 字段（缺失或类型不符返回 None）
fn get_i64(v: &serde_json::Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64())
}

/// 解析字段缺失时的时间戳兜底（系统时间）
fn fallback_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造一份贴近 B 站真实结构的 DANMU_MSG（info 下标映射见 parse_danmaku 注释）
    fn sample_danmu(content: serde_json::Value) -> serde_json::Value {
        let mut info: Vec<serde_json::Value> = vec![
            // 0: [mode, 弹幕类型, fontsize, 颜色, 时间戳(ms), 随机id]
            serde_json::json!([0, 1, 25, 16750848, 1700000123456i64, 98765]),
            // 1: 内容（字符串或 [字符串]）
            content,
            // 2: [uid, uname, 是否房管]
            serde_json::json!([10086, "测试用户", 1]),
            // 3: [勋章等级, 勋章名, 主播名, 勋章房间id]
            serde_json::json!([20, "测试牌", "主播名", 9999]),
            // 4: [用户等级, ...]
            serde_json::json!([42, 0, 0, ">50000"]),
            serde_json::json!([]),  // 5
            serde_json::json!([]),  // 6
            serde_json::json!(3),   // 7: 舰队等级（3=舰长 2=提督 1=总督）
        ];
        // 8..=15 填充空数组，使 16 号位为荣耀等级
        for _ in 8..16 {
            info.push(serde_json::json!([]));
        }
        info.push(serde_json::json!([7])); // 16: [荣耀等级]
        serde_json::json!({ "cmd": "DANMU_MSG", "info": info })
    }

    fn parse_danmu(v: serde_json::Value) -> Danmaku {
        let payload = v.to_string();
        let events = parse_payload(payload.as_bytes()).expect("payload 应可解析");
        let mut danmu = events.into_iter().filter_map(|e| match e {
            BilibiliEvent::Danmaku(d) => Some(d),
            BilibiliEvent::Backing(_) => None,
        });
        danmu.next().expect("应产出一条弹幕")
    }

    /// 解析出唯一一条打赏（无则 panic）
    fn parse_backing(v: serde_json::Value) -> Backing {
        let payload = v.to_string();
        let events = parse_payload(payload.as_bytes()).expect("payload 应可解析");
        let mut backings = events.into_iter().filter_map(|e| match e {
            BilibiliEvent::Backing(b) => Some(b),
            BilibiliEvent::Danmaku(_) => None,
        });
        backings.next().expect("应产出一条打赏")
    }

    /// SEND_GIFT 样本：10 万金瓜子的付费礼物（"火箭"）
    fn sample_gift() -> serde_json::Value {
        serde_json::json!({
            "cmd": "SEND_GIFT",
            "data": {
                "giftName": "火箭",
                "num": 1,
                "uname": "老板A",
                "uid": 10086,
                "timestamp": 1700000200i64,
                "giftId": 10001,
                "price": 100000,
                "total_coin": 100000,
                "coin_type": "gold",
                "rnd": "abc123",
                "guard_level": 3,
                "wealth_level": 20,
                "medal_info": {
                    "medal_level": 20,
                    "medal_name": "测试牌",
                    "medal_room_id": 9999
                }
            }
        })
    }

    #[test]
    fn 礼物金额折算为分() {
        let b = parse_backing(sample_gift());
        assert_eq!(b.kind, BackingKind::Gift);
        assert_eq!(b.gift_name, "火箭");
        assert_eq!(b.username, "老板A");
        assert_eq!(b.num, 1);
        // 100000 金瓜子 = 100 元 = 10000 分
        assert_eq!(b.amount_fen, 10000);
        assert_eq!(b.id, "gift-abc123", "有 rnd 时用它做去重标识");
        assert_eq!(b.medal_level, Some(20));
        assert_eq!(b.medal_room_id, Some(9999));
        assert_eq!(b.guard_level, Some(3));
        assert_eq!(b.wealth_level, Some(20));
    }

    #[test]
    fn 银瓜子礼物被丢弃() {
        let mut v = sample_gift();
        v["data"]["coin_type"] = serde_json::json!("silver");
        let events = parse_payload(v.to_string().as_bytes()).unwrap();
        assert!(events.is_empty(), "免费礼物不应产生打赏事件");
    }

    #[test]
    fn 零价礼物被丢弃() {
        let mut v = sample_gift();
        v["data"]["price"] = serde_json::json!(0);
        v["data"]["total_coin"] = serde_json::json!(0);
        v["data"]["coin_type"] = serde_json::json!("");
        let events = parse_payload(v.to_string().as_bytes()).unwrap();
        assert!(events.is_empty(), "单价与总价都为 0 视为免费礼物");
    }

    #[test]
    fn 礼物缺总价时按单价乘数量() {
        let mut v = sample_gift();
        v["data"].as_object_mut().unwrap().remove("total_coin");
        v["data"]["num"] = serde_json::json!(3);
        let b = parse_backing(v);
        // 100000 金瓜子 x 3 = 300 元 = 30000 分
        assert_eq!(b.amount_fen, 30000);
    }

    #[test]
    fn 连击信号不计金额() {
        let v = serde_json::json!({
            "cmd": "COMBO_SEND",
            "data": {
                "uname": "老板A",
                "uid": 10086,
                "gift_name": "火箭",
                "gift_id": 10001,
                "combo_num": 5,
                "timestamp": 1700000201i64
            }
        });
        let b = parse_backing(v);
        assert_eq!(b.kind, BackingKind::Combo);
        assert_eq!(b.amount_fen, 0, "金额一律以 SEND_GIFT 为准，避免重复计数");
        assert_eq!(b.num, 0);
        assert_eq!(b.gift_id, 10001);
        assert_eq!(b.gift_name, "火箭");
    }

    #[test]
    fn 醒目留言按人民币元折算() {
        let v = serde_json::json!({
            "cmd": "SUPER_CHAT_MESSAGE",
            "data": {
                "id": 555,
                "uid": 10086,
                "user_info": { "uname": "老板B", "wealth_level": 30 },
                "message": "加油",
                "price": 30,
                "start_time": 1700000300i64,
                "guard_level": 2,
                "medal_info": {
                    "medal_level": 10,
                    "medal_name": "牌子",
                    "anchor_roomid": 9999
                }
            }
        });
        let b = parse_backing(v);
        assert_eq!(b.kind, BackingKind::SuperChat);
        // price 已是人民币元，30 元 = 3000 分
        assert_eq!(b.amount_fen, 3000);
        assert_eq!(b.gift_name, "醒目留言");
        assert_eq!(b.message.as_deref(), Some("加油"));
        assert_eq!(b.username, "老板B");
        assert_eq!(b.id, "sc-555");
        assert_eq!(b.medal_room_id, Some(9999), "兼容 anchor_roomid 写法");
        assert_eq!(b.wealth_level, Some(30));
    }

    #[test]
    fn 上舰缺礼物名时按档位补() {
        let v = serde_json::json!({
            "cmd": "GUARD_BUY",
            "data": {
                "uid": 10086,
                "username": "老板C",
                "guard_level": 1,
                "num": 1,
                "price": 1998000,
                "gift_id": 10003,
                "start_time": 1700000400i64
            }
        });
        let b = parse_backing(v);
        assert_eq!(b.kind, BackingKind::Guard);
        assert_eq!(b.gift_name, "总督");
        assert_eq!(b.username, "老板C");
        // 1998000 金瓜子 = 1998 元 = 199800 分
        assert_eq!(b.amount_fen, 199800);
    }

    // ---- SEND_GIFT_V2（protobuf 载荷）----

    /// 最小编码器：varint 字段（V2 字段号都很小，单字节 key 够用）
    fn pb_int(field: u32, value: u64) -> Vec<u8> {
        let mut out = vec![(field << 3) as u8];
        let mut v = value;
        while v >= 0x80 {
            out.push((v as u8 & 0x7F) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
        out
    }

    /// 最小编码器：长度前缀字段
    fn pb_bytes(field: u32, data: &[u8]) -> Vec<u8> {
        let mut out = vec![((field << 3) | 2) as u8];
        let mut len = data.len() as u64;
        while len >= 0x80 {
            out.push((len as u8 & 0x7F) | 0x80);
            len >>= 7;
        }
        out.push(len as u8);
        out.extend_from_slice(data);
        out
    }

    /// 构造一个 SendGiftV2GiftItem
    fn gift_item(
        gift_id: u64,
        name: &str,
        num: u64,
        price: u64,
        total_coin: u64,
        coin_type: &str,
        timestamp: u64,
        rnd: &str,
    ) -> Vec<u8> {
        [
            pb_int(1, gift_id),
            pb_bytes(2, name.as_bytes()),
            pb_int(3, num),
            pb_int(5, price),
            pb_int(7, total_coin),
            pb_bytes(8, coin_type.as_bytes()),
            pb_int(10, timestamp),
            pb_bytes(12, rnd.as_bytes()),
        ]
        .concat()
    }

    /// 构造一条完整的 SEND_GIFT_V2（顶层用户信息 + 若干礼物档位）
    fn v2_payload(items: Vec<Vec<u8>>) -> serde_json::Value {
        let medal = [
            pb_int(4, 9999),
            pb_int(5, 20),
            pb_bytes(6, "测试牌".as_bytes()),
        ]
        .concat();
        let mut top = [
            pb_int(1, 10086),
            pb_bytes(2, "老板A".as_bytes()),
            pb_int(5, 3),
            pb_bytes(8, &medal),
        ]
        .concat();
        for item in items {
            top.extend(pb_bytes(10, &item));
        }
        serde_json::json!({
            "cmd": "SEND_GIFT_V2",
            "data": { "pb": protobuf::encode_base64(&top) }
        })
    }

    #[test]
    fn v2礼物按金瓜子折算为分() {
        let v = v2_payload(vec![gift_item(
            10001,
            "火箭",
            1,
            100000,
            100000,
            "gold",
            1700000200,
            "v2rnd",
        )]);
        let b = parse_backing(v);
        assert_eq!(b.kind, BackingKind::Gift);
        assert_eq!(b.gift_name, "火箭");
        assert_eq!(b.username, "老板A");
        assert_eq!(b.num, 1);
        assert_eq!(b.amount_fen, 10000, "100000 金瓜子 = 100 元");
        assert_eq!(b.id, "gift-v2rnd");
        assert_eq!(b.timestamp, 1700000200);
        assert_eq!(b.guard_level, Some(3));
        assert_eq!(b.medal_level, Some(20));
        assert_eq!(b.medal_name.as_deref(), Some("测试牌"));
        assert_eq!(b.medal_room_id, Some(9999));
        assert_eq!(b.wealth_level, None, "V2 广播不带荣耀等级");
    }

    #[test]
    fn v2银瓜子礼物被丢弃() {
        let v = v2_payload(vec![gift_item(
            1,
            "粉丝团灯牌",
            1,
            0,
            0,
            "silver",
            1700000200,
            "x",
        )]);
        let events = parse_payload(v.to_string().as_bytes()).unwrap();
        assert!(events.is_empty(), "免费礼物不应产生打赏事件");
    }

    #[test]
    fn v2一条消息多个礼物档位() {
        let v = v2_payload(vec![
            gift_item(10001, "火箭", 1, 100000, 100000, "gold", 1700000200, "a"),
            gift_item(10002, "小心心", 5, 10, 50, "gold", 1700000200, "b"),
        ]);
        let events = parse_payload(v.to_string().as_bytes()).unwrap();
        let names: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                BilibiliEvent::Backing(b) => Some(b.gift_name.clone()),
                BilibiliEvent::Danmaku(_) => None,
            })
            .collect();
        assert_eq!(names, vec!["火箭", "小心心"]);
    }

    #[test]
    fn v2坏载荷不panic() {
        for pb in ["", "!!!", "Zm9v"] {
            let v = serde_json::json!({ "cmd": "SEND_GIFT_V2", "data": { "pb": pb } });
            assert!(
                parse_payload(v.to_string().as_bytes()).unwrap().is_empty(),
                "载荷 {pb:?} 应被安静跳过"
            );
        }
        // 缺 data.pb
        let v = serde_json::json!({ "cmd": "SEND_GIFT_V2", "data": {} });
        assert!(parse_payload(v.to_string().as_bytes()).unwrap().is_empty());
    }

    #[test]
    fn 完整字段映射() {
        let d = parse_danmu(sample_danmu(serde_json::json!("测试弹幕")));
        assert_eq!(d.id, "1700000123-10086-98765");
        assert_eq!(d.content, "测试弹幕");
        assert_eq!(d.username, "测试用户");
        assert_eq!(d.timestamp, 1700000123, "毫秒时间戳应转秒");
        assert_eq!(d.color.as_deref(), Some("#FF9900"));
        assert_eq!(d.medal_level, Some(20));
        assert_eq!(d.medal_name.as_deref(), Some("测试牌"));
        assert_eq!(d.medal_room_id, Some(9999));
        assert_eq!(d.user_level, Some(42));
        assert_eq!(d.guard_level, Some(3));
        assert_eq!(d.wealth_level, Some(7));
        assert!(d.is_admin, "房管标记应从 info[2][2] 解析");
    }

    #[test]
    fn 内容为数组形态() {
        let d = parse_danmu(sample_danmu(serde_json::json!(["数组弹幕"])));
        assert_eq!(d.content, "数组弹幕");
    }

    #[test]
    fn 秒级时间戳不除千() {
        let mut v = sample_danmu(serde_json::json!("x"));
        v["info"][0][4] = serde_json::json!(1700000123); // 已是秒
        let d = parse_danmu(v);
        assert_eq!(d.timestamp, 1700000123);
    }

    #[test]
    fn 字段缺失容错() {
        let info = serde_json::json!([
            [0, 1, 25, 16777215, 1700000123456i64, 1],
            "只有内容",
            [],
            []
        ]);
        let d = parse_danmu(serde_json::json!({ "cmd": "DANMU_MSG", "info": info }));
        assert_eq!(d.content, "只有内容");
        assert_eq!(d.username, "");
        assert_eq!(d.timestamp, 1700000123);
        assert!(!d.is_admin);
    }

    #[test]
    fn 空内容被丢弃() {
        let v = sample_danmu(serde_json::json!("   "));
        let payload = v.to_string();
        let events = parse_payload(payload.as_bytes()).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn 非弹幕命令被忽略() {
        let payload = serde_json::json!({ "cmd": "NOTICE_MSG" }).to_string();
        let events = parse_payload(payload.as_bytes()).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn 非utf8或坏json返回错误() {
        assert!(parse_payload(b"\xff\xfe").is_err());
        assert!(parse_payload(b"not json").is_err());
    }
}
