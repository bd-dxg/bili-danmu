//! B 站直播弹幕发送（网页端 msg/send 协议）
//!
//! 仅登录态可用：发送需要 Cookie 中的 bili_jct（CSRF token），
//! 游客无 bili_jct，接口返回 code=-101（未登录）。

use std::time::{SystemTime, UNIX_EPOCH};

/// 发送弹幕到指定直播间
///
/// `room_id` 为真实房间号（短号需先经 resolve_room 解析）；`cookies` 为登录 Cookie 串。
pub async fn send_danmaku(
    client: &reqwest::Client,
    cookies: &str,
    room_id: u32,
    msg: &str,
) -> Result<(), String> {
    // CSRF token：bili_jct（登录时由 Cookie 签发，发送弹幕签名必需）
    let csrf = crate::bilibili::login::cookie_value(cookies, "bili_jct")
        .ok_or_else(|| "登录 Cookie 缺少 bili_jct，请重新扫码登录".to_string())?;

    // rnd：毫秒时间戳（接口防重放参数）
    let rnd = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
        .to_string();
    let room_id = room_id.to_string();

    let resp = client
        .post("https://api.live.bilibili.com/msg/send")
        .header("Cookie", cookies)
        .form(&[
            ("bubble", "0"),
            ("msg", msg),
            ("color", "16777215"), // 白色 0xFFFFFF
            ("mode", "1"),         // 1=滚动弹幕
            ("fontsize", "25"),
            ("rnd", &rnd),
            ("roomid", &room_id),
            ("csrf", &csrf),
            ("csrf_token", &csrf),
        ])
        .send()
        .await
        .map_err(|e| format!("发送弹幕请求失败: {e}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("发送弹幕响应解析失败: {e}"))?;

    let code = resp.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        let message = resp.get("message").and_then(|m| m.as_str()).unwrap_or("");
        return Err(format!("发送失败(code={code}): {message}"));
    }
    Ok(())
}
