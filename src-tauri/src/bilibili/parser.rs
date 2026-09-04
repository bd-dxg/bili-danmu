//! B 站弹幕协议：消息展开（解压）与 JSON 命令解析
//!
//! 服务器下发链：网络帧(Op5, proto 0/2/3)
//!   → proto 0: body 即 JSON（可能为多个命令的数组）
//!   → proto 2/3: body 是 Zlib/Brotli 压缩的嵌套包流，解压后递归拆包

use crate::bilibili::event::{BilibiliEvent, Danmaku};
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
            "" => { /* 无 cmd 的消息体忽略 */ }
            other => events.push(BilibiliEvent::Other(other.to_string())),
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

/// 解析字段缺失时的时间戳兑底（系统时间）
/// 解析字段缺失时的时间戳兑底（系统时间）
fn fallback_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
