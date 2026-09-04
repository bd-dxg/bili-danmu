//! B 站扫码登录（网页端 passport 协议）
//!
//! 现状（2025+）：B 站要求登录态才会推送弹幕，游客 WS 收不到 DANMU_MSG。
//! 流程：generate 获取二维码 → 前端展示 → poll 轮询扫码结果
//!   → 成功后从响应 Set-Cookie 提取登录 Cookie 持久化。

use reqwest::header::SET_COOKIE;

/// 二维码数据
#[derive(Debug, Clone, serde::Serialize)]
pub struct QrData {
    /// 二维码内容（登录确认 URL）
    pub url: String,
    /// 轮询凭据
    pub key: String,
}

/// 轮询状态
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "status")]
pub enum PollResult {
    /// 等待扫码
    Scanning,
    /// 已扫码待确认
    Confirmed,
    /// 二维码过期，需重新生成
    Expired,
    /// 登录成功（附带登录 Cookie 串）
    Success { cookies: String },
    /// 其他错误
    Error { message: String },
}

/// 生成登录二维码
pub async fn qr_generate(client: &reqwest::Client) -> Result<QrData, String> {
    let resp = client
        .get("https://passport.bilibili.com/x/passport-login/web/qrcode/generate")
        .send()
        .await
        .map_err(|e| format!("请求登录二维码失败: {e}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("二维码响应解析失败: {e}"))?;

    let code = resp.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(format!(
            "生成二维码失败(code={code}): {}",
            resp.get("message").and_then(|m| m.as_str()).unwrap_or("")
        ));
    }
    let data = resp
        .get("data")
        .ok_or_else(|| "二维码响应缺少 data".to_string())?;
    Ok(QrData {
        url: data
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        key: data
            .get("qrcode_key")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

/// 轮询扫码结果
pub async fn qr_poll(client: &reqwest::Client, key: &str) -> Result<PollResult, String> {
    let url = format!("https://passport.bilibili.com/x/passport-login/web/qrcode/poll?qrcode_key={key}");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("轮询扫码状态失败: {e}"))?;

    // 先提取 Set-Cookie（json() 会消费响应）
    let set_cookies: Vec<String> = resp
        .headers()
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok().map(String::from))
        .collect();

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("轮询响应解析失败: {e}"))?;

    let outer = json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if outer != 0 {
        return Ok(PollResult::Error {
            message: format!("接口错误 code={outer}"),
        });
    }
    // 真实扫码状态在 data.code（外层 code=0 仅表示接口正常）
    let status = json
        .get("data")
        .and_then(|d| d.get("code"))
        .and_then(|c| c.as_i64())
        .unwrap_or(-1);
    match status {
        0 => {
            // 登录成功：从 Set-Cookie 提取登录 Cookie
            let mut cookies = String::new();
            for s in set_cookies {
                if let Some(eq) = s.find('=') {
                    let end = s[eq + 1..].find(';').map(|i| eq + 1 + i).unwrap_or(s.len());
                    cookies.push_str(&s[..end]);
                    cookies.push_str("; ");
                }
            }
            if cookies.is_empty() {
                return Ok(PollResult::Error {
                    message: "登录成功但未获取到 Cookie".into(),
                });
            }
            Ok(PollResult::Success { cookies })
        }
        86038 => Ok(PollResult::Expired),
        86101 => Ok(PollResult::Scanning),
        86090 => Ok(PollResult::Confirmed),
        other => Ok(PollResult::Error {
            message: format!("扫码状态异常 data.code={other}"),
        }),
    }
}

/// 从 Cookie 串中提取指定 cookie 的值
pub fn cookie_value(cookies: &str, name: &str) -> Option<String> {
    for part in cookies.split(';') {
        let part = part.trim();
        if let Some((n, v)) = part.split_once('=') {
            if n.trim() == name {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

/// 登录后通过 nav 接口获取用户昵称（带登录 Cookie）
pub async fn fetch_uname(client: &reqwest::Client, cookies: &str) -> Result<String, String> {
    let resp = client
        .get("https://api.bilibili.com/x/web-interface/nav")
        .header("Cookie", cookies)
        .send()
        .await
        .map_err(|e| format!("请求用户信息失败: {e}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("用户信息响应解析失败: {e}"))?;

    let code = resp.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(format!("nav 接口 code={code}"));
    }
    let uname = resp
        .pointer("/data/uname")
        .and_then(|u| u.as_str())
        .unwrap_or("")
        .to_string();
    if uname.is_empty() {
        Err("nav 响应缺少昵称".into())
    } else {
        Ok(uname)
    }
}
