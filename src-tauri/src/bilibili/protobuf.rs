//! 极简 protobuf 解码 + base64 拆包
//!
//! 只服务一件事：`SEND_GIFT_V2` 的载荷是 base64 编码的 protobuf（SendGiftBroadcast），
//! 而依赖里没有 protobuf 库，也不想为一条消息引入 prost（还要本机装 protoc）。
//! 所以这里只实现 wire type 0/1/2/5 的遍历，够读出礼物列表即可。
//!
//! 字段号与 SEND_GIFT_V2 的对应关系（括号内为 wire type）：
//! ```text
//! SendGiftBroadcast
//!   1  uid (0)   2  uname (2)   3  face (2)   5  guard_level (0)
//!   8  medal_info (2)   9  blind_gift (2)   10 gift_list (2, 可重复)
//! SendGiftV2GiftItem
//!   1  gift_id (0)   2  gift_name (2)   3  num (0)   4  gift_type (0)
//!   5  price (0)   7  total_coin (0)   8  coin_type (2)   9  tid (2)
//!   10 timestamp (0)   12 rnd (2)   18 action (2)   35 gift_info (2)
//! SendGiftV2MedalInfo
//!   1  target_id (0)   4  anchor_roomid (0)   5  medal_level (0)   6  medal_name (2)
//! ```

/// 一层消息里读到的字段值
#[derive(Debug, Clone, Copy)]
pub enum Value<'a> {
    /// wire type 0：varint（int / bool / enum）
    Int(u64),
    /// wire type 2：长度前缀（string / bytes / 嵌套消息 / 重复字段）
    Bytes(&'a [u8]),
    /// wire type 1/5：定长数字，本项目用不到，已跳过
    Skipped,
}

/// 解析一层消息 → (字段号, 值) 列表，按出现顺序；重复字段会出现多次
pub fn decode(data: &[u8]) -> Result<Vec<(u32, Value<'_>)>, String> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let key = varint(data, &mut pos)?;
        let field = (key >> 3) as u32;
        let value = match (key & 0x07) as u8 {
            0 => Value::Int(varint(data, &mut pos)?),
            1 => {
                pos = advance(data, pos, 8)?;
                Value::Skipped
            }
            2 => {
                let len = varint(data, &mut pos)? as usize;
                let end = advance(data, pos, len)?;
                let bytes = &data[pos..end];
                pos = end;
                Value::Bytes(bytes)
            }
            5 => {
                pos = advance(data, pos, 4)?;
                Value::Skipped
            }
            other => return Err(format!("protobuf wire type {other} 不支持")),
        };
        out.push((field, value));
    }
    Ok(out)
}

/// 前移 n 字节并检查越界
fn advance(data: &[u8], pos: usize, n: usize) -> Result<usize, String> {
    pos.checked_add(n)
        .filter(|end| *end <= data.len())
        .ok_or_else(|| "protobuf 字段越界".to_string())
}

/// 读取一个 varint
fn varint(data: &[u8], pos: &mut usize) -> Result<u64, String> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *data.get(*pos).ok_or("protobuf varint 越界")?;
        *pos += 1;
        value |= ((byte & 0x7F) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("protobuf varint 过长".into())
}

/// 取第一个 int 字段
pub fn int(fields: &[(u32, Value<'_>)], no: u32) -> Option<u64> {
    fields.iter().find_map(|(f, v)| match (f == &no, v) {
        (true, Value::Int(n)) => Some(*n),
        _ => None,
    })
}

/// 取第一个 bytes 字段
pub fn bytes<'a>(fields: &[(u32, Value<'a>)], no: u32) -> Option<&'a [u8]> {
    fields.iter().find_map(|(f, v)| match (f == &no, v) {
        (true, Value::Bytes(b)) => Some(*b),
        _ => None,
    })
}

/// 取第一个 string 字段（非法 UTF-8 用替换字符兜底，不报错）
pub fn string(fields: &[(u32, Value<'_>)], no: u32) -> Option<String> {
    bytes(fields, no).map(|b| String::from_utf8_lossy(b).into_owned())
}

/// 取全部 bytes 字段（重复字段，如 gift_list）
pub fn bytes_all<'a>(fields: &[(u32, Value<'a>)], no: u32) -> Vec<&'a [u8]> {
    fields
        .iter()
        .filter_map(|(f, v)| match (f == &no, v) {
            (true, Value::Bytes(b)) => Some(*b),
            _ => None,
        })
        .collect()
}

const BASE64_TABLE: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 标准 base64 解码（忽略空白字符；遇非法字符返回 None）
pub fn decode_base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        if c.is_ascii_whitespace() {
            continue;
        }
        let v = BASE64_TABLE.iter().position(|t| *t == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

/// 标准 base64 编码（仅供测试构造样本）
#[cfg(test)]
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(BASE64_TABLE[((n >> (18 - i * 6)) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小编码器：varint 字段（测试用）
    fn pb_int(field: u32, value: u64) -> Vec<u8> {
        let mut out = vec![((field << 3) | 0) as u8];
        let mut v = value;
        while v >= 0x80 {
            out.push((v as u8 & 0x7F) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
        out
    }

    /// 最小编码器：长度前缀字段（测试用）
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

    #[test]
    fn 解析varint与长度前缀字段() {
        let raw = [pb_int(1, 300), pb_bytes(2, "火箭".as_bytes())].concat();
        let fields = decode(&raw).unwrap();
        assert_eq!(int(&fields, 1), Some(300));
        assert_eq!(string(&fields, 2).as_deref(), Some("火箭"));
    }

    #[test]
    fn 重复字段全部读出() {
        let raw = [
            pb_bytes(10, b"a"),
            pb_bytes(10, b"b"),
            pb_bytes(10, b"c"),
        ]
        .concat();
        let fields = decode(&raw).unwrap();
        assert_eq!(bytes_all(&fields, 10).len(), 3);
    }

    #[test]
    fn 定长与未知wiretype不panic() {
        // wire type 1（8 字节定长）跳过，后续字段仍能读出
        let mut raw = vec![(3 << 3 | 1) as u8];
        raw.extend_from_slice(&[0u8; 8]);
        raw.extend_from_slice(&pb_int(4, 7));
        let fields = decode(&raw).unwrap();
        assert_eq!(int(&fields, 4), Some(7));
        assert!(matches!(fields[0].1, Value::Skipped));
        // wire type 3（已废弃）返回错误而不是 panic
        assert!(decode(&[0x1Bu8]).is_err());
    }

    #[test]
    fn 截断数据返回错误() {
        assert!(decode(&[0x0A, 0x05, b'a']).is_err(), "长度超出剩余数据应报错");
        assert!(decode(&[0x80]).is_err(), "varint 未收尾应报错");
    }

    #[test]
    fn base64往返() {
        for case in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", "火箭".as_bytes()] {
            let encoded = encode_base64(case);
            assert_eq!(decode_base64(&encoded).unwrap(), case, "{encoded}");
        }
        assert_eq!(decode_base64("Zm9vYg==").unwrap(), b"foob");
        assert!(decode_base64("!!!!").is_none());
    }
}
