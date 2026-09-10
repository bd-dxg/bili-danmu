//! B 站直播 HTTP 接口：房间解析、弹幕服务器配置、主播信息
//!
//! 弹幕服务器的 WebSocket 会话见 `ws.rs`。

use std::time::Duration;

const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

#[derive(Debug, Clone)]
pub struct ResolvedRoom {
    /// 真实房间号（弹幕服务使用）
    pub room_id: u32,
    /// 0=未开播 1=直播中 2=轮播
    pub live_status: i64,
}

#[derive(Debug, Clone)]
pub struct DanmuConf {
    pub token: String,
    pub hosts: Vec<HostInfo>,
}

#[derive(Debug, Clone)]
pub struct HostInfo {
    pub host: String,
    pub wss_port: u16,
}

/// 构建带浏览器 UA/Referer/Origin 的 HTTP 客户端
///
/// 请求头三件套（UA/Referer/Origin）缺一不可：B 站直播接口对无浏览器化请求头的
/// 请求会返回 code:-352（已登录却提示 cookie 无效多由此而来，参考 DanmuFree 注释）。
/// 注意不启用 cookie_store：登录 Cookie 由登录流程手动提取后全程经显式 Cookie
/// header 传递（见 qr_poll / fetch_danmu_conf），避免 store 与手动 header 语义混淆。
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(UA)
        .default_headers({
            let mut h = reqwest::header::HeaderMap::new();
            h.insert(
                reqwest::header::REFERER,
                reqwest::header::HeaderValue::from_static("https://live.bilibili.com/"),
            );
            h.insert(
                reqwest::header::ORIGIN,
                reqwest::header::HeaderValue::from_static("https://live.bilibili.com"),
            );
            h
        })
        .timeout(Duration::from_secs(10))
        .build()
        .expect("构建 HTTP 客户端失败")
}

/// 短号 → 真实房间号 + 直播状态
pub async fn resolve_room(client: &reqwest::Client, short_id: u32) -> Result<ResolvedRoom, String> {
    let url = format!("https://api.live.bilibili.com/room/v1/Room/room_init?id={short_id}");
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("请求房间信息失败: {e}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("房间信息响应解析失败: {e}"))?;

    let code = resp.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(format!(
            "房间不存在或不可用(code={code}): {}",
            resp.get("message").and_then(|m| m.as_str()).unwrap_or("")
        ));
    }
    let data = resp
        .get("data")
        .ok_or_else(|| "房间信息缺少 data".to_string())?;
    let room_id = data
        .get("room_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or_else(|| "房间信息缺少 room_id".to_string())?;
    let live_status = data.get("live_status").and_then(|v| v.as_i64()).unwrap_or(0);
    Ok(ResolvedRoom {
        room_id,
        live_status,
    })
}

/// 获取房间主播昵称（匿名可请求；失败返回 None，调用方回退显示房间号）
pub async fn fetch_anchor_uname(client: &reqwest::Client, room_id: u32) -> Option<String> {
    let url = format!(
        "https://api.live.bilibili.com/live_user/v1/UserInfo/get_anchor_in_room?roomid={room_id}"
    );
    let resp = client
        .get(&url)
        .send()
        .await
        .ok()?
        .json::<serde_json::Value>()
        .await
        .ok()?;
    if resp.get("code").and_then(|c| c.as_i64()).unwrap_or(-1) != 0 {
        return None;
    }
    let uname = resp.pointer("/data/info/uname")?.as_str()?.trim().to_string();
    if uname.is_empty() {
        None
    } else {
        Some(uname)
    }
}

/// 获取弹幕服务器配置（token + host 列表）
///
/// 返回 (DanmuConf, guest_mode)：guest_mode=true 表示 token 为游客级，
/// 认证时必须使用 uid=0（否则服务器会因 token/uid 不匹配直接断开）。
///
/// - 登录态（cookies 非空）：getDanmuInfo 需 WBI 签名（w_rid），缺签名即 -352；
/// - 游客 / 登录路径失败：回退旧版 getConf（游客级 token）。
pub async fn fetch_danmu_conf(
    client: &reqwest::Client,
    room_id: u32,
    cookies: &str,
) -> Result<(DanmuConf, bool), String> {
    if !cookies.trim().is_empty() {
        match fetch_via_danmu_info_signed(client, room_id, cookies).await {
            Ok(conf) => return Ok((conf, false)),
            Err(e) => eprintln!("[bilibili] getDanmuInfo(wbi) 失败: {e}，回退游客 getConf"),
        }
    }
    let conf = fetch_via_get_conf(client, room_id).await?;
    Ok((conf, true))
}

/// getDanmuInfo（WBI 签名 + 登录 cookie），返回登录级 token
async fn fetch_via_danmu_info_signed(
    client: &reqwest::Client,
    room_id: u32,
    cookies: &str,
) -> Result<DanmuConf, String> {
    // 1. nav 拿 wbi img_key/sub_key（匿名可得，约每日更替，每次现取）
    let nav: serde_json::Value = client
        .get("https://api.bilibili.com/x/web-interface/nav")
        .header("Cookie", cookies)
        .send()
        .await
        .map_err(|e| format!("请求 nav 失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("nav 响应解析失败: {e}"))?;
    let (img_key, sub_key) =
        crate::bilibili::wbi::parse_wbi_keys(&nav).ok_or_else(|| "nav 缺少 wbi_img".to_string())?;
    let mixin = crate::bilibili::wbi::mixin_key(&img_key, &sub_key)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let signed = crate::bilibili::wbi::sign(&[("id", &room_id.to_string()), ("type", "0")], &mixin, now);

    // 2. 带签名请求 getDanmuInfo
    let url = format!(
        "https://api.live.bilibili.com/xlive/web-room/v1/index/getDanmuInfo?{signed}"
    );
    let resp: serde_json::Value = client
        .get(&url)
        .header("Cookie", cookies)
        .send()
        .await
        .map_err(|e| format!("请求弹幕配置失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("弹幕配置响应解析失败: {e}"))?;

    let code = resp.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(format!("code={code}"));
    }
    let data = resp
        .get("data")
        .ok_or_else(|| "弹幕配置缺少 data".to_string())?;
    let token = data
        .get("token")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();
    let hosts = parse_host_list(data.get("host_list"));
    if hosts.is_empty() {
        return Err("host_list 为空".into());
    }
    Ok(DanmuConf { token, hosts })
}

/// 旧版 getConf（无风控、无需 cookie）
async fn fetch_via_get_conf(
    client: &reqwest::Client,
    room_id: u32,
) -> Result<DanmuConf, String> {
    let url = format!(
        "https://api.live.bilibili.com/room/v1/Danmu/getConf?room_id={room_id}&platform=pc&player=web"
    );
    let resp = client
        .get(&url)
        .header("Referer", format!("https://live.bilibili.com/{room_id}"))
        .send()
        .await
        .map_err(|e| format!("请求弹幕配置(getConf)失败: {e}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("弹幕配置(getConf)响应解析失败: {e}"))?;

    let code = resp.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(format!(
            "获取弹幕配置(getConf)失败(code={code}): {}",
            resp.get("message").and_then(|m| m.as_str()).unwrap_or("")
        ));
    }
    let data = resp
        .get("data")
        .ok_or_else(|| "弹幕配置缺少 data".to_string())?;
    let token = data
        .get("token")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    // host_server_list 优先，兼容旧 host 字段
    let mut hosts = parse_host_list(data.get("host_server_list"));
    if hosts.is_empty() {
        if let Some(h) = data.get("host").and_then(|x| x.as_str()) {
            hosts.push(HostInfo {
                host: h.to_string(),
                wss_port: 443,
            });
        }
    }
    if hosts.is_empty() {
        return Err("host_server_list 为空".into());
    }
    Ok(DanmuConf { token, hosts })
}

/// 从接口响应中解析 host 列表（兼容 host_list / host_server_list 两种字段名）
fn parse_host_list(v: Option<&serde_json::Value>) -> Vec<HostInfo> {
    let mut hosts = Vec::new();
    if let Some(list) = v.and_then(|h| h.as_array()) {
        for h in list {
            if let (Some(host), Some(port)) = (
                h.get("host").and_then(|v| v.as_str()),
                h.get("wss_port").and_then(|v| v.as_u64()),
            ) {
                hosts.push(HostInfo {
                    host: host.to_string(),
                    wss_port: port as u16,
                });
            }
        }
    }
    hosts
}
