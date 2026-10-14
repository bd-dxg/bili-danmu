//! Edge TTS 的 DRM 令牌 `Sec-MS-GEC`（v2）
//!
//! 微软 2026 年 10 月起强制 v2：令牌从「纯 SHA256」变成 `<16 hex>-<64 hex>`（CS64 + 另一套
//! 改过常数的 SHA256），版本串同步从 `1-` 变 `2-`。v1 令牌已在国内等地区被 403 拒绝
//! （握手 `Invalid response status`），故整体切到 v2。
//!
//! 算法逐行照抄官方 edge-tts 的 `src/edge_tts/drm.py`：两套哈希的常数都被刻意改过，
//! **不能**换成标准库的 SHA256。单测用官方抓包向量锁定，见 `v2_token_matches_captured_vector`。
//!
//! 待哈希的 ticks 字符串必须与官方的浮点路径一致（`unix + 纪元 → 取整到 5 分钟 → ×1e7`），
//! 改成「更干净」的整数运算会让某些时间戳的最后几位错开，令牌随即失配。
//!
//! 本机时钟偏差（`CLOCK_SKEW_MS`）由 `edge.rs` 在握手被 403 拒绝时按响应头 `Date` 校准，
//! 与官方 `DRM.clock_skew_seconds` 同一语义。

#[cfg(test)]
use std::sync::atomic::AtomicU32;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::util::days_from_civil;
use super::TRUSTED_CLIENT_TOKEN;

/// Windows FILETIME 纪元（1601-01-01）相对 Unix 纪元的秒数
const WIN_EPOCH: f64 = 11_644_473_600.0;
const U32_MASK: u64 = 0xFFFF_FFFF;

const SHA256_INITIAL: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB,
    0x5BE0CD19,
];
const SHA256_STATE: [u32; 8] = [
    0xFD9FBEA2, 0x91243CE7, 0xBE499094, 0xCD2E1E0A, 0x6C006F49, 0x04BD35AB, 0xFEB0A7E4,
    0x03EFFBD2,
];
const SHA256_SCHEDULE: [u32; 14] = [
    0xEB24BF9E, 0x1558691F, 0xA0C09EE2, 0x0CFA32BA, 0xA622EE8C, 0xD1DD9491, 0xC0DD25A0,
    0xF9E70AB6, 0xF0C31573, 0x7058CE4E, 0x7188E606, 0x00E5AB33, 0xD258B060, 0x72766564,
];
/// 标准 SHA-256 的轮常数，仅第 15 项（下标 14）被替换成 [`SHA256_ADJUSTMENT`]
const SHA256_ROUND_CONSTANTS: [u32; 64] = [
    0x428A2F98, 0x71374491, 0xB5C0FBCF, 0xE9B5DBA5, 0x3956C25B, 0x59F111F1, 0x923F82A4,
    0xAB1C5ED5, 0xD807AA98, 0x12835B01, 0x243185BE, 0x550C7DC3, 0x72BE5D74, 0x80DEB1FE,
    0x9BDC06A7, 0xC19BF174, 0xE49B69C1, 0xEFBE4786, 0x0FC19DC6, 0x240CA1CC, 0x2DE92C6F,
    0x4A7484AA, 0x5CB0A9DC, 0x76F988DA, 0x983E5152, 0xA831C66D, 0xB00327C8, 0xBF597FC7,
    0xC6E00BF3, 0xD5A79147, 0x06CA6351, 0x14292967, 0x27B70A85, 0x2E1B2138, 0x4D2C6DFC,
    0x53380D13, 0x650A7354, 0x766A0ABB, 0x81C2C92E, 0x92722C85, 0xA2BFE8A1, 0xA81A664B,
    0xC24B8B70, 0xC76C51A3, 0xD192E819, 0xD6990624, 0xF40E3585, 0x106AA070, 0x19A4C116,
    0x1E376C08, 0x2748774C, 0x34B0BCB5, 0x391C0CB3, 0x4ED8AA4A, 0x5B9CCA4F, 0x682E6FF3,
    0x748F82EE, 0x78A5636F, 0x84C87814, 0x8CC70208, 0x90BEFFFA, 0xA4506CEB, 0xBEF9A3F7,
    0xC67178F2,
];
const SHA256_ADJUSTMENT: u32 = 0xC9DC06A7;
const SHA256_SIGMA0_ADJUSTMENT: u32 = 0x059C0B80;
const SHA256_SIGMA1_ADJUSTMENT: u32 = 0x000B92C0;

/// CS64 的 14 组初始（系数, 偏移）
const CS64_COEFFICIENTS: [(u32, u32); 14] = [
    (0xE82E91B1, 0xB32700D0),
    (0x44DE24C6, 0x88DFF51D),
    (0x3BEB65BC, 0x29514038),
    (0xC89C5933, 0xB328CBDF),
    (0x8D1EC211, 0xD405B4D5),
    (0x314EF5D5, 0x26E55F3C),
    (0xE92491B1, 0x607801C6),
    (0xF0A5B9D5, 0x82B2C1FB),
    (0x06064CF3, 0xFA51A3DA),
    (0xDA79FA60, 0x207CC464),
    (0xBED20560, 0x3C3C74F3),
    (0x89F9C9F8, 0xD2D14B61),
    (0x35FA38C6, 0xBB04A2C1),
    (0xCB49E3C6, 0xC5626167),
];
const CS64_FACTOR: u32 = 0x82893933;
const CS64_ADJUSTMENT0: u32 = 0xF2F90B1A;
const CS64_ADJUSTMENT1: u32 = 0x0B1A0000;

/// 32 位左旋 16 位
fn rol16(value: u32) -> u32 {
    value.rotate_left(16)
}

/// SHA-256 小 sigma0（保留高位，调用处再截断）
fn sigma0(value: u32) -> u32 {
    let v = value as u64;
    (((v >> 7) ^ (v << 25) ^ (v >> 18) ^ (v << 14) ^ (v >> 3)) & U32_MASK) as u32
}

/// SHA-256 小 sigma1（保留高位，调用处再截断）
fn sigma1(value: u32) -> u32 {
    let v = value as u64;
    (((v >> 17) ^ (v << 15) ^ (v >> 19) ^ (v << 13) ^ (v >> 10)) & U32_MASK) as u32
}

/// 拆成小端 32 位字，尾部补零到 4 字节对齐；字数为奇数时再补一个 0（CS64 要求成对处理）
fn words_le4_even(data: &[u8]) -> Vec<u32> {
    let mut padded = data.to_vec();
    while padded.len() % 4 != 0 {
        padded.push(0);
    }
    let mut words: Vec<u32> = padded
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if words.len() % 2 != 0 {
        words.push(0);
    }
    words
}

/// 扩充 SHA-256 消息表（sigma 先合并再取模，与标准写法不同）
fn expand_schedule(words: &mut Vec<u32>, first_word: usize) {
    for index in first_word..64 {
        let x = words[index - 2] as u64;
        let y = words[index - 15] as u64;
        let s1 = (x >> 17) ^ (x << 15) ^ (x >> 19) ^ (x << 13) ^ (x >> 10);
        let s0 = (y >> 7) ^ (y << 25) ^ (y >> 18) ^ (y << 14) ^ (y >> 3);
        let value = (s1 + words[index - 7] as u64 + s0 + words[index - 16] as u64) & U32_MASK;
        words.push(value as u32);
    }
}

/// SHA-256 压缩函数。`first_round` 为 14 时用被替换过常数的那张表（`_sha256_init` 的续算）
fn sha256_rounds(state: [u32; 8], words: &[u32], first_round: usize) -> [u32; 8] {
    let m = U32_MASK;
    let block = first_round == 14;
    let constant = |index: usize| -> u64 {
        if block && index == 14 {
            SHA256_ADJUSTMENT as u64
        } else {
            SHA256_ROUND_CONSTANTS[index] as u64
        }
    };
    // 在 64 位里重复一份再右移，等价于 ROTR 且省掉左移那一步
    let rotr = |x: u64| (x >> 6) ^ (x >> 11) ^ (x >> 25);
    let big_sigma0 = |x: u64| (x >> 2) ^ (x >> 13) ^ (x >> 22);

    let mut a = state[0] as u64;
    let mut b = state[1] as u64;
    let mut c = state[2] as u64;
    let mut d = state[3] as u64;
    let mut e = state[4] as u64;
    let mut f = state[5] as u64;
    let mut g = state[6] as u64;
    let mut h = state[7] as u64;

    let aligned = std::cmp::min(64, ((first_round + 7) / 8) * 8);
    for index in first_round..aligned {
        let rot_e = e * 0x1_0000_0001;
        let rot_a = a * 0x1_0000_0001;
        let t1 = h
            + rotr(rot_e)
            + (g ^ (e & (f ^ g)))
            + constant(index)
            + words[index] as u64;
        let t2 = big_sigma0(rot_a) + ((a & b) | (c & (a | b)));
        let na = (t1 + t2) & m;
        let ne = (d + t1) & m;
        // 官方写法：八轮的寄存器名各回原位，中间这几轮整体平移
        h = g;
        g = f;
        f = e;
        e = ne;
        d = c;
        c = b;
        b = a;
        a = na;
    }

    // 其余轮次按八轮展开，寄存器靠「读谁、写谁」承担角色轮转
    let mut index = aligned;
    while index < 64 {
        let rot_e = e * 0x1_0000_0001;
        let rot_a = a * 0x1_0000_0001;
        let t1 =
            h + rotr(rot_e) + (g ^ (e & (f ^ g))) + constant(index) + words[index] as u64;
        d = (d + t1) & m;
        h = (t1 + big_sigma0(rot_a) + ((a & b) | (c & (a | b)))) & m;

        let rot_e = d * 0x1_0000_0001;
        let rot_a = h * 0x1_0000_0001;
        let t1 = g
            + rotr(rot_e)
            + (f ^ (d & (e ^ f)))
            + constant(index + 1)
            + words[index + 1] as u64;
        c = (c + t1) & m;
        g = (t1 + big_sigma0(rot_a) + ((h & a) | (b & (h | a)))) & m;

        let rot_e = c * 0x1_0000_0001;
        let rot_a = g * 0x1_0000_0001;
        let t1 = f
            + rotr(rot_e)
            + (e ^ (c & (d ^ e)))
            + constant(index + 2)
            + words[index + 2] as u64;
        b = (b + t1) & m;
        f = (t1 + big_sigma0(rot_a) + ((g & h) | (a & (g | h)))) & m;

        let rot_e = b * 0x1_0000_0001;
        let rot_a = f * 0x1_0000_0001;
        let t1 = e
            + rotr(rot_e)
            + (d ^ (b & (c ^ d)))
            + constant(index + 3)
            + words[index + 3] as u64;
        a = (a + t1) & m;
        e = (t1 + big_sigma0(rot_a) + ((f & g) | (h & (f | g)))) & m;

        let rot_e = a * 0x1_0000_0001;
        let rot_a = e * 0x1_0000_0001;
        let t1 = d
            + rotr(rot_e)
            + (c ^ (a & (b ^ c)))
            + constant(index + 4)
            + words[index + 4] as u64;
        h = (h + t1) & m;
        d = (t1 + big_sigma0(rot_a) + ((e & f) | (g & (e | f)))) & m;

        let rot_e = h * 0x1_0000_0001;
        let rot_a = d * 0x1_0000_0001;
        let t1 = c
            + rotr(rot_e)
            + (b ^ (h & (a ^ b)))
            + constant(index + 5)
            + words[index + 5] as u64;
        g = (g + t1) & m;
        c = (t1 + big_sigma0(rot_a) + ((d & e) | (f & (d | e)))) & m;

        let rot_e = g * 0x1_0000_0001;
        let rot_a = c * 0x1_0000_0001;
        let t1 = b
            + rotr(rot_e)
            + (a ^ (g & (h ^ a)))
            + constant(index + 6)
            + words[index + 6] as u64;
        f = (f + t1) & m;
        b = (t1 + big_sigma0(rot_a) + ((c & d) | (e & (c | d)))) & m;

        let rot_e = f * 0x1_0000_0001;
        let rot_a = b * 0x1_0000_0001;
        let t1 = a
            + rotr(rot_e)
            + (h ^ (f & (g ^ h)))
            + constant(index + 7)
            + words[index + 7] as u64;
        e = (e + t1) & m;
        a = (t1 + big_sigma0(rot_a) + ((b & c) | (d & (b | c)))) & m;

        index += 8;
    }

    [
        a as u32, b as u32, c as u32, d as u32, e as u32, f as u32, g as u32, h as u32,
    ]
}

/// 令牌数据的前 7 字节定出初始状态，第 14 轮起接着算（官方把标准常数表切开了）
fn sha256_init(data: &[u8]) -> [u32; 8] {
    let mut words: Vec<u32> = vec![0; 14];
    words.push(u32::from_be_bytes([0, data[0], data[1], data[2]]));
    words.push(u32::from_be_bytes([data[3], data[4], data[5], data[6]]));
    for index in 16..30usize {
        let mut value = SHA256_SCHEDULE[index - 16] as u64;
        if index - 2 >= 14 {
            let s1 = sigma1(words[index - 2]) as u64;
            value += if index == 16 {
                s1 ^ SHA256_SIGMA1_ADJUSTMENT as u64
            } else {
                s1
            };
        }
        if index - 7 >= 14 {
            value += words[index - 7] as u64;
        }
        if index - 15 >= 14 {
            let s0 = sigma0(words[index - 15]) as u64;
            value += if index == 29 {
                s0 ^ SHA256_SIGMA0_ADJUSTMENT as u64
            } else {
                s0
            };
        }
        words.push((value & U32_MASK) as u32);
    }
    let sum = sigma1(words[28]) as u64
        + words[23] as u64
        + sigma0(words[15]) as u64
        + words[14] as u64
        + SHA256_ADJUSTMENT as u64;
    words.push((sum.wrapping_sub(SHA256_ROUND_CONSTANTS[14] as u64) & U32_MASK) as u32);
    expand_schedule(&mut words, 31);
    let working = sha256_rounds(SHA256_STATE, &words, 14);
    let mut state = [0u32; 8];
    for i in 0..8 {
        state[i] = SHA256_INITIAL[i].wrapping_add(working[i]);
    }
    state
}

/// 非标准的 SHA-256：前 7 字节喂进 [`sha256_init`]，其余按官方补位规则哈希
fn sha256_digest(data: &[u8]) -> [u8; 32] {
    assert!(data.len() >= 7, "令牌数据至少 7 字节");
    let length = 57usize + data.len();
    let mut padded = data[7..].to_vec();
    padded.push(0x80);
    let zeros = (55i64 - length as i64).rem_euclid(64) as usize;
    padded.extend(std::iter::repeat(0u8).take(zeros));
    padded.extend_from_slice(&((length as u64) * 8).to_be_bytes());

    let mut state = sha256_init(&data[..7]);
    for chunk in padded.chunks_exact(64) {
        let mut words: Vec<u32> = chunk
            .chunks_exact(4)
            .map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        expand_schedule(&mut words, 16);
        let working = sha256_rounds(state, &words, 0);
        for i in 0..8 {
            state[i] = state[i].wrapping_add(working[i]);
        }
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&state[i].to_be_bytes());
    }
    out
}

/// CS64 摘要：把 14 组系数与令牌字成对折叠回来
fn cs64_digest(data: &[u8], digest: &[u8; 32]) -> (u32, u32) {
    let m = U32_MASK;
    let key_a = (u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]) | 1) as u64;
    let key_b = (u32::from_le_bytes([digest[4], digest[5], digest[6], digest[7]]) | 1) as u64;
    let factors = [key_a * CS64_FACTOR as u64, key_b * CS64_FACTOR as u64];

    let mut terms: Vec<u32> = CS64_COEFFICIENTS
        .iter()
        .enumerate()
        .map(|(i, (coefficient, _))| {
            (*coefficient as u128 * factors[i % 2] as u128 & m as u128) as u32
        })
        .collect();
    let mut offsets: Vec<u32> = CS64_COEFFICIENTS.iter().map(|(_, offset)| *offset).collect();

    // 前置一个 0 字节再拆字（与小端拆分的奇偶对齐有关，照抄官方）
    let mut buf = vec![0u8];
    buf.extend_from_slice(data);
    let word_list = words_le4_even(&buf);
    // 成对折叠，字数必须为偶数（`words_le4_even` 已保证）
    debug_assert_eq!(word_list.len() % 2, 0);
    for (index, &word) in word_list.iter().enumerate() {
        let key = if index % 2 == 0 { key_a } else { key_b };
        let mut term = word as u128 * key as u128;
        let mut offset = rol16(word) as u64
            * if index % 2 == 0 {
                0xEF0569FB
            } else {
                0xC31713DB
            };
        if index == 0 {
            term += CS64_ADJUSTMENT0 as u128 * factors[0] as u128;
            offset += CS64_ADJUSTMENT1 as u64;
        }
        terms.push((term & m as u128) as u32);
        offsets.push((offset & m) as u32);
    }

    let (mut swap_acc, mut swap_last, mut state, mut acc) = (0u64, 0u64, 0u64, 0u64);
    let mut index = 0;
    while index < terms.len() {
        let x = terms[index] as u64;
        let y = terms[index + 1] as u64;

        let t = rol16(((offsets[index] as u64 + x) & m) as u32) as u64;
        let q = t.wrapping_mul(0x76F50F34) & m;
        swap_acc += (rol16(q as u32) as u64 * 0xC3EFEA97 + q) & m;

        let t = rol16(((offsets[index + 1] as u64 + y) & m) as u32) as u64;
        let q = t.wrapping_mul(0x1881CE3C) & m;
        swap_last = (rol16(q as u32) as u64 * 0x35BD1EC9 + q) & m;
        swap_acc = (swap_acc + swap_last) & m;

        state = (state.wrapping_mul(key_a).wrapping_add(x)) & m;
        state = rol16(state as u32) as u64 * 0xCF98B111 & m;
        state = rol16(state as u32) as u64 * 0x87085B9F & m;
        state = rol16(state as u32) as u64 * 0x12CEB96D & m;
        state = rol16(state as u32) as u64 * 0x257E1D83 & m;
        acc = (acc + state) & m;

        let mut t = (state.wrapping_mul(key_b).wrapping_add(y)) & m;
        t = rol16(t as u32) as u64 * 0xA27416F5 & m;
        t = rol16(t as u32) as u64 * 0xD38396FF & m;
        let inner = rol16(t as u32) as u64 * 0x7C932B89 & m;
        state = rol16(inner as u32) as u64 * 0xBFA49F69 & m;
        acc = (acc + state) & m;

        index += 2;
    }

    ((swap_last ^ state) as u32, (swap_acc ^ acc) as u32)
}

fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// v2 令牌：`<CS64 的 16 位 hex>-<SHA256 的 64 位 hex>`
fn token_at(unix_timestamp: f64) -> String {
    let mut ticks = unix_timestamp + WIN_EPOCH;
    ticks -= ticks % 300.0;
    ticks *= 1e7;
    let data = format!("{ticks:.0}{TRUSTED_CLIENT_TOKEN}");
    let bytes = data.as_bytes();
    let digest = sha256_digest(bytes);
    let (high, low) = cs64_digest(bytes, &digest);
    let mut cs64 = Vec::with_capacity(8);
    cs64.extend_from_slice(&high.to_le_bytes());
    cs64.extend_from_slice(&low.to_le_bytes());
    format!("{}-{}", hex_upper(&cs64), hex_upper(&digest))
}

/// 本机时钟与服务端的偏差（毫秒）：握手被 403 拒绝时按响应头 `Date` 校准，
/// 与官方 `DRM.clock_skew_seconds` 同一语义（累加而非覆盖）
static CLOCK_SKEW_MS: AtomicI64 = AtomicI64::new(0);

/// 校准次数，仅测试用于断言 403 重试路径真的走到（生产不计数）
#[cfg(test)]
static SKEW_CALIBRATIONS: AtomicU32 = AtomicU32::new(0);

/// 按服务端 `Date` 头校准本机时钟偏差；返回是否解析成功（失败时偏差保持不变）
pub fn adjust_clock_skew(server_date: &str) -> bool {
    let Some(server_secs) = parse_rfc1123(server_date) else {
        return false;
    };
    let delta_ms = ((server_secs - unix_secs()) * 1000.0).round() as i64;
    CLOCK_SKEW_MS.fetch_add(delta_ms, Ordering::Relaxed);
    #[cfg(test)]
    SKEW_CALIBRATIONS.fetch_add(1, Ordering::Relaxed);
    true
}

/// 直接覆盖本机时钟偏差（仅测试：用于人为模拟偏差超窗）
#[cfg(test)]
pub fn set_clock_skew_seconds(secs: f64) {
    CLOCK_SKEW_MS.store((secs * 1000.0) as i64, Ordering::Relaxed);
}

/// 累计校准次数（仅测试）
#[cfg(test)]
pub fn skew_calibrations() -> u32 {
    SKEW_CALIBRATIONS.load(Ordering::Relaxed)
}

/// 当前 Unix 秒（已加时钟偏差修正）
fn unix_secs() -> f64 {
    raw_unix_secs() + CLOCK_SKEW_MS.load(Ordering::Relaxed) as f64 / 1000.0
}

fn raw_unix_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// 解析 HTTP `Date` 头的 RFC 1123 形式（服务端实测发的就是这种：`Wed, 21 Oct 2015 07:28:00 GMT`）
fn parse_rfc1123(date: &str) -> Option<f64> {
    let (_, rest) = date.trim().split_once(", ")?;
    let mut parts = rest.split_whitespace();
    let day: u32 = parts.next()?.parse().ok()?;
    let month = month_from_abbr(parts.next()?)?;
    let year: i64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let second: i64 = clock.next()?.parse().ok()?;
    if parts.next()? != "GMT" {
        return None;
    }
    Some((days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second) as f64)
}

fn month_from_abbr(abbr: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MONTHS.iter().position(|m| *m == abbr).map(|i| i as u32 + 1)
}

/// 当前时刻的 `Sec-MS-GEC` 令牌（发 `Sec-MS-GEC-Version: 2-…` 时必须用它）
pub fn sec_ms_gec() -> String {
    token_at(unix_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 官方 PR（rany2/edge-tts#492）里从 Edge 抓包得到的向量，逐字节锁定两套哈希
    #[test]
    fn v2_token_matches_captured_vector() {
        assert_eq!(
            token_at(1_791_888_000.0),
            "492D7694A183A45E-\
             56E821F95D4579C5B7C7E33DC7303FF379EC36D510213555BBBB9EB17EF15876"
        );
    }

    #[test]
    fn token_shape() {
        let token = sec_ms_gec();
        let (cs64, sha256) = token.split_once('-').expect("必须有分隔符");
        assert_eq!(cs64.len(), 16);
        assert_eq!(sha256.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
        assert!(!token.chars().any(|c| c.is_ascii_lowercase()));
    }

    #[test]
    fn token_is_stable_inside_five_minute_window() {
        // 同一 5 分钟窗口内的两个时刻必须生成同一个令牌
        let base = 1_791_888_000.0;
        assert_eq!(token_at(base), token_at(base + 299.0));
        assert_ne!(token_at(base), token_at(base + 300.0));
    }

    #[test]
    fn rfc1123_date_parses() {
        assert_eq!(
            parse_rfc1123("Wed, 21 Oct 2015 07:28:00 GMT"),
            Some(1_445_412_480.0)
        );
        assert_eq!(parse_rfc1123("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0.0));
        assert_eq!(parse_rfc1123("Thu, 01 Jan 1970 00:00:00 UTC"), None);
        assert_eq!(parse_rfc1123("not a date"), None);
        assert_eq!(parse_rfc1123(""), None);
    }

    #[test]
    fn clock_skew_shifts_current_time() {
        let before = unix_secs();
        // 服务端时间远早于本机 → 偏差为负，时间应被拉回去
        assert!(adjust_clock_skew("Thu, 01 Jan 1970 00:00:00 GMT"));
        assert!(unix_secs() < before);
        assert!(!adjust_clock_skew("垃圾"));
        // 还原，避免并行测试互相干扰
        CLOCK_SKEW_MS.store(0, Ordering::Relaxed);
        assert!((unix_secs() - before).abs() < 5.0);
    }
}
