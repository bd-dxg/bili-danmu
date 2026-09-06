//! B 站 WBI 签名（w_rid）
//!
//! 参考 DanmuFree 的实现与 bilibili-API-collect 逆向文档：
//! ① img_key/sub_key 取自 nav 接口 data.wbi_img.{img_url,sub_url} 文件名（去 .png），约每日更替；
//! ② mixin_key = 按 MixinKeyEncTab 重排 img_key+sub_key 取前 32 位；
//! ③ 加 wts(unix 秒)，参数按 key 升序拼 query，w_rid = MD5(query + mixin_key) 小写 hex。
//!
//! 实测（与 DanmuFree 一致）：getDanmuInfo 自 2026-08 起强制 WBI 鉴权，
//! 缺 w_rid 即返回 code:-352，即使已带登录 cookie + 浏览器化请求头。

/// 固定重排映射表（64 项，全站统一）
pub const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

/// img_key + sub_key → mixin_key
///
/// 重排表索引覆盖 0..=63，要求拼接后至少 64 字节（B 站现为 32+32 字符）；
/// 长度不足说明上游格式已变化，返回错误而非索引越界 panic。
pub fn mixin_key(img_key: &str, sub_key: &str) -> Result<String, String> {
    let raw = format!("{img_key}{sub_key}");
    let bytes = raw.as_bytes();
    if bytes.len() < 64 {
        return Err(format!(
            "wbi key 长度异常（img={img_key} sub={sub_key}），可能接口已改版"
        ));
    }
    Ok(MIXIN_KEY_ENC_TAB[..32]
        .iter()
        .map(|&i| bytes[i] as char)
        .collect())
}

/// 对参数做 WBI 签名，返回可直接拼到 URL 的 query（含 w_rid）
pub fn sign(parameters: &[(&str, &str)], mixin: &str, unix_now: i64) -> String {
    let wts = unix_now.to_string();
    let mut sorted: Vec<(&str, String)> = parameters
        .iter()
        .map(|(k, v)| (*k, v.to_string()))
        .collect();
    sorted.push(("wts", wts));
    sorted.sort_by(|a, b| a.0.cmp(b.0));

    let mut query = String::new();
    for (k, v) in &sorted {
        if !query.is_empty() {
            query.push('&');
        }
        query.push_str(k);
        query.push('=');
        query.push_str(&encode(v));
    }
    // 过滤 !'()*（WBI 规范）
    let filtered: String = query.chars().filter(|c| !matches!(c, '!' | '\'' | '(' | ')' | '*')).collect();
    let digest = md5_hex(format!("{filtered}{mixin}").as_bytes());
    format!("{filtered}&w_rid={digest}")
}

/// encodeURIComponent 兼容
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn md5_hex(data: &[u8]) -> String {
    use md5::Digest;
    let digest = md5::Md5::digest(data);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// 从 nav 接口响应解析 img_key / sub_key
pub fn parse_wbi_keys(nav: &serde_json::Value) -> Option<(String, String)> {
    let img_url = nav
        .pointer("/data/wbi_img/img_url")?
        .as_str()?;
    let sub_url = nav
        .pointer("/data/wbi_img/sub_url")?
        .as_str()?;
    Some((key_from_url(img_url), key_from_url(sub_url)))
}

fn key_from_url(url: &str) -> String {
    let name = url.rsplit('/').next().unwrap_or("");
    name.strip_suffix(".png").unwrap_or(name).to_string()
}
