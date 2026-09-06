//! B 站直播弹幕 WebSocket 二进制协议（封包/解包）
//!
//! 包格式（大端序）：
//! ```text
//! 0-3   总长度（含 16 字节头）
//! 4-5   头长度（固定 16）
//! 6-7   协议版本: 0=JSON 1=人气 2=Zlib 3=Brotli
//! 8-11  操作码: 2=心跳 3=人气回 5=命令 7=认证 8=认证成功
//! 12-15 序号
//! 16+   包体
//! ```

pub const OP_HEARTBEAT: u32 = 2;
pub const OP_MESSAGE: u32 = 5;
pub const OP_AUTH: u32 = 7;
pub const OP_AUTH_REPLY: u32 = 8;

pub const PROTO_JSON: u16 = 0;
pub const PROTO_ZLIB: u16 = 2;
pub const PROTO_BROTLI: u16 = 3;

pub const HEADER_LEN: usize = 16;

/// 单包上限（解压后数据过大视为异常，防呆）
pub const MAX_PACKET_LEN: usize = 4 * 1024 * 1024;
/// 单次网络读入拆包数量上限（解压嵌套不应产生海量包）
pub const MAX_PACKETS_PER_READ: usize = 4096;

#[derive(Debug, Clone)]
pub struct Packet {
    pub proto: u16,
    pub op: u32,
    pub body: Vec<u8>,
}

/// 构造一个包（用于认证、心跳）
pub fn encode_packet(op: u32, body: &[u8]) -> Vec<u8> {
    let total = HEADER_LEN + body.len();
    let mut p = Vec::with_capacity(total);
    p.extend_from_slice(&(total as u32).to_be_bytes());
    p.extend_from_slice(&(HEADER_LEN as u16).to_be_bytes());
    p.extend_from_slice(&0u16.to_be_bytes()); // proto 0
    p.extend_from_slice(&op.to_be_bytes());
    p.extend_from_slice(&1u32.to_be_bytes()); // 序号
    p.extend_from_slice(body);
    p
}

/// 解析单个完整包（调用方保证 buf 长度 >= total）
pub fn decode_packet(buf: &[u8]) -> Result<Packet, String> {
    if buf.len() < HEADER_LEN {
        return Err("包长度不足 16 字节头".into());
    }
    let total = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    let header = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let proto = u16::from_be_bytes([buf[6], buf[7]]);
    let op = u32::from_be_bytes([buf[8], buf[9], buf[10], buf[11]]);

    if total < HEADER_LEN || total > MAX_PACKET_LEN {
        return Err(format!("非法包总长度 {total}"));
    }
    if header < HEADER_LEN || header > total {
        return Err(format!("非法包头长度 {header}"));
    }
    Ok(Packet {
        proto,
        op,
        body: buf[header..total].to_vec(),
    })
}

/// 从一个字节流中拆出连续的多个包（网络帧可能一次携带多包）
pub fn decode_stream(buf: &[u8]) -> Result<Vec<Packet>, String> {
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + HEADER_LEN <= buf.len() {
        let total = u32::from_be_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]) as usize;
        if total < HEADER_LEN || off + total > buf.len() {
            // 尾部数据不完整（B 站按包发送，正常不会出现）：记录并丢弃，
            // 不静默吞掉——若频繁出现说明协议行为已变化，便于排查
            eprintln!("[protocol] 帧尾含不完整/畸形包，丢弃 {} 字节", buf.len() - off);
            break;
        }
        out.push(decode_packet(&buf[off..off + total])?);
        off += total;
        if out.len() >= MAX_PACKETS_PER_READ {
            break;
        }
    }
    Ok(out)
}
