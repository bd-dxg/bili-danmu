//! B 站弹幕协议：消息展开（解压）与 JSON 命令分发
//!
//! 服务器下发链：网络帧(Op5, proto 0/2/3)
//!   → proto 0: body 即 JSON（可能为多个命令的数组）
//!   → proto 2/3: body 是 Zlib/Brotli 压缩的嵌套包流，解压后递归拆包
//!
//! 命令解析按域拆在子模块（本文件只留解压展开、`parse_payload` 分发与共用小工具）：
//! `danmaku.rs` 弹幕 / `gift.rs` 打赏 / `welcome.rs` 欢迎。单测在本文件末尾。

mod danmaku;
mod gift;
mod welcome;

use crate::bilibili::event::BilibiliEvent;
use crate::bilibili::protocol::{decode_stream, Packet, PROTO_BROTLI, PROTO_JSON, PROTO_ZLIB};
use brotli_decompressor::Decompressor;
use danmaku::parse_danmaku;
use flate2::read::ZlibDecoder;
use gift::{parse_combo, parse_gift, parse_gift_v2, parse_guard_buy, parse_super_chat};
use std::io::Read;
use welcome::{parse_entry_effect, parse_interact_word_v2, parse_like_click};

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
            // ---- 欢迎类（进房 / 关注 / 分享 / 点赞 / 舰长进场）----
            "INTERACT_WORD_V2" => {
                if let Some(w) = parse_interact_word_v2(item) {
                    events.push(BilibiliEvent::Welcome(w));
                }
            }
            "LIKE_INFO_V3_CLICK" => {
                if let Some(w) = parse_like_click(item) {
                    events.push(BilibiliEvent::Welcome(w));
                }
            }
            "ENTRY_EFFECT" => {
                if let Some(w) = parse_entry_effect(item) {
                    events.push(BilibiliEvent::Welcome(w));
                }
            }
            // 其它命令（在线榜 / 看过人数 / 全网广播 / LIKE_INFO_V3_UPDATE 等）
            // 暂无消费者，直接忽略——逐条构造事件并打印日志会在高频事件流下
            // 造成日志洪泛与无谓分配
            _ => {}
        }
    }
    Ok(events)
}

/// 时间戳归一化：13 位毫秒转秒，0 或缺失时用系统时间。
/// 各命令一律走这里，避免某条路径忘了转毫秒写出错误时间。
pub(super) fn normalize_timestamp(t: i64) -> i64 {
    if t <= 0 {
        return fallback_now();
    }
    if t > 10_000_000_000 {
        t / 1000
    } else {
        t
    }
}

/// 金瓜子 → 分（1000 金瓜子 = 1 元 = 100 分）
pub(super) fn coin_to_fen(coin: i64) -> u64 {
    (coin.max(0) as u64) / 10
}

/// 读取字符串字段（缺失或类型不符返回 None）
pub(super) fn get_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(String::from)
}

/// 读取 u32 字段（缺失或类型不符返回 None）
pub(super) fn get_u32(v: &serde_json::Value, key: &str) -> Option<u32> {
    v.get(key).and_then(|x| x.as_u64()).map(|n| n as u32)
}

/// 读取 i64 字段（缺失或类型不符返回 None）
pub(super) fn get_i64(v: &serde_json::Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64())
}

/// 解析字段缺失时的时间戳兜底（系统时间）
pub(super) fn fallback_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 下面这些符号现在只被测试用到（非测试代码里 parse_payload 只产出 BilibiliEvent），
    // import 就放在测试模块里，避免生产构建出现 unused_imports 警告
    use crate::bilibili::event::{Backing, BackingKind, Danmaku, Welcome, WelcomeKind};
    use crate::bilibili::protobuf;

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

    /// 在标准样本上补出 `info[0][13]`（表情位）
    ///
    /// 测试样本的 info[0] 只有 6 位（真实报文有到 17 位），补齐到 14 位后才能写下标 13；
    /// 中间位填字符串 `"{}"`，与真实报文里「无表情」的形态一致。
    fn with_emote(mut v: serde_json::Value, emote: serde_json::Value) -> serde_json::Value {
        let head = v["info"][0].as_array_mut().expect("info[0] 应为数组");
        while head.len() < 14 {
            head.push(serde_json::json!("{}"));
        }
        head[13] = emote;
        v
    }

    fn parse_danmu(v: serde_json::Value) -> Danmaku {
        let payload = v.to_string();
        let events = parse_payload(payload.as_bytes()).expect("payload 应可解析");
        let mut danmu = events.into_iter().filter_map(|e| match e {
            BilibiliEvent::Danmaku(d) => Some(d),
            _ => None,
        });
        danmu.next().expect("应产出一条弹幕")
    }

    /// 解析出唯一一条打赏（无则 panic）
    fn parse_backing(v: serde_json::Value) -> Backing {
        let payload = v.to_string();
        let events = parse_payload(payload.as_bytes()).expect("payload 应可解析");
        let mut backings = events.into_iter().filter_map(|e| match e {
            BilibiliEvent::Backing(b) => Some(b),
            _ => None,
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
                "wealth_level": 20
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
                "guard_level": 2
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
        let mut top = [
            pb_int(1, 10086),
            pb_bytes(2, "老板A".as_bytes()),
            pb_int(5, 3),
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
                _ => None,
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
    fn 表情弹幕解析出表情信息() {
        // info[0][13] 实测形态：UP主大表情（room_，bulge_display=1，162×162）
        let v = with_emote(
            sample_danmu(serde_json::json!("生气")),
            serde_json::json!({
                "bulge_display": 1,
                "emoticon_unique": "room_22637261_34519",
                "height": 162,
                "is_dynamic": 1,
                "url": "https://i0.hdslb.com/bfs/garb/92855a2c.png",
                "width": 162
            }),
        );
        let d = parse_danmu(v);
        let e = d.emote.expect("应解析出表情");
        assert_eq!(e.unique, "room_22637261_34519");
        assert_eq!(e.width, 162);
        assert_eq!(e.height, 162);
        assert!(e.bulge, "bulge_display=1 应标记为大表情");
        // 正文就是表情名：不带中括号的「生气」也照样是表情弹幕
        assert_eq!(d.content, "生气");
    }

    // 表情字段本身的边界（无字段 / 空唯一号 / 缺地址 / 地址归一化 / 正文到离）
    // 已在 `emote.rs` 用直接调用覆盖，这里只保留一条走完整 DANMU_MSG 的集成测试

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

    // ---- 欢迎类（字段号来自实测，见 Task/findings.md）----

    /// 构造一条 INTERACT_WORD_V2（msg_type: 1 进入 / 2 关注 / 3 分享）
    fn interact_word(msg_type: u64, uid: u64, uname: &str, ts: u64) -> serde_json::Value {
        let pb = [
            pb_int(1, uid),
            pb_bytes(2, uname.as_bytes()),
            pb_int(5, msg_type),
            pb_int(6, 1978001213),
            pb_int(7, ts),
        ]
        .concat();
        serde_json::json!({
            "cmd": "INTERACT_WORD_V2",
            "data": { "dmscore": 3, "pb": protobuf::encode_base64(&pb) }
        })
    }

    /// 解析出唯一一条欢迎事件（无则 panic）
    fn parse_welcome(v: serde_json::Value) -> Welcome {
        let payload = v.to_string();
        let events = parse_payload(payload.as_bytes()).expect("payload 应可解析");
        events
            .into_iter()
            .find_map(|e| match e {
                BilibiliEvent::Welcome(w) => Some(w),
                _ => None,
            })
            .expect("应产出一条欢迎事件")
    }

    #[test]
    fn 进房关注分享按msg_type区分() {
        assert_eq!(
            parse_welcome(interact_word(1, 10086, "观众A", 1700000300)).kind,
            WelcomeKind::Enter
        );
        assert_eq!(
            parse_welcome(interact_word(2, 10086, "观众A", 1700000300)).kind,
            WelcomeKind::Follow
        );
        assert_eq!(
            parse_welcome(interact_word(3, 10086, "观众A", 1700000300)).kind,
            WelcomeKind::Share
        );
    }

    #[test]
    fn 进房事件带上用户名与时间戳() {
        let w = parse_welcome(interact_word(1, 10086, "观众A", 1700000300));
        assert_eq!(w.uid, 10086);
        assert_eq!(w.username, "观众A");
        assert_eq!(w.timestamp, 1700000300);
        assert_eq!(w.guard_level, None, "进房事件里没有 guard_level");
    }

    #[test]
    fn 未知msg_type的进房事件被丢弃() {
        let v = interact_word(9, 10086, "观众A", 1700000300);
        let events = parse_payload(v.to_string().as_bytes()).unwrap();
        assert!(events.is_empty(), "msg_type=9 不猜语义，直接丢");
    }

    #[test]
    fn 缺pb的进房事件不panic() {
        let v = serde_json::json!({ "cmd": "INTERACT_WORD_V2", "data": {} });
        let events = parse_payload(v.to_string().as_bytes()).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn 点赞事件是单条而不是总数() {
        let v = serde_json::json!({
            "cmd": "LIKE_INFO_V3_CLICK",
            "data": {
                "uid": 19702780,
                "uname": "卡皮巴拉吃豆檛子",
                "msg_type": 6,
                "uinfo": { "guard": { "level": 0 } }
            }
        });
        let w = parse_welcome(v);
        assert_eq!(w.kind, WelcomeKind::Like);
        assert_eq!(w.username, "卡皮巴拉吃豆檛子");
        assert_eq!(w.guard_level, None, "guard.level=0 应归一成 None");
    }

    #[test]
    fn 点赞总数事件不产生欢迎行() {
        // LIKE_INFO_V3_UPDATE 只有 click_count，是累计值不是事件
        let v = serde_json::json!({
            "cmd": "LIKE_INFO_V3_UPDATE",
            "data": { "click_count": 79652 }
        });
        let events = parse_payload(v.to_string().as_bytes()).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn 舰长进场事件带出舰队等级() {
        let v = serde_json::json!({
            "cmd": "ENTRY_EFFECT",
            "data": {
                "id": 380,
                "uid": 79648849,
                "business": 6,
                "copy_writing": "<%舰长甲%> 来了",
                "uinfo": {
                    "uid": 79648849,
                    "base": { "name": "舰长甲" },
                    "guard": { "level": 3 }
                }
            }
        });
        let w = parse_welcome(v);
        assert_eq!(w.kind, WelcomeKind::GuardEnter);
        assert_eq!(w.username, "舰长甲");
        assert_eq!(w.guard_level, Some(3));
    }

    #[test]
    fn 普通高等级观众进场不算上舰() {
        // 实测样本：ENTRY_EFFECT 也会给荣耀等级高的普通观众触发（guard.level=0）
        let v = serde_json::json!({
            "cmd": "ENTRY_EFFECT",
            "data": {
                "id": 380,
                "uid": 79648849,
                "copy_writing": "<%观众甲%> 来了",
                "uinfo": {
                    "uid": 79648849,
                    "base": { "name": "观众甲" },
                    "guard": { "level": 0 },
                    "wealth": { "level": 15 }
                }
            }
        });
        let events = parse_payload(v.to_string().as_bytes()).unwrap();
        assert!(events.is_empty(), "guard.level=0 不是上舰欢迎");
    }
}
