//! B 站直播间连接：HTTP 鉴权 + WebSocket 会话
//!
//! 流程：短号 → 真实房间号(room_init) → 弹幕服务器配置(getDanmuInfo, 需 buvid3 cookie)
//!   → wss 连接 → 认证包(protover=3) → 30s 心跳 → 收弹幕事件

use crate::bilibili::event::BilibiliEvent;
use crate::bilibili::parser;
use crate::bilibili::protocol::{self, OP_AUTH, OP_AUTH_REPLY, OP_HEARTBEAT, OP_MESSAGE};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tauri::Emitter;
use tokio_util::sync::CancellationToken;

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

/// 构建带 cookie 存储与浏览器 UA/Referer/Origin 的 HTTP 客户端
///
/// 请求头三件套（UA/Referer/Origin）缺一不可：B 站直播接口对无浏览器化请求头的
/// 请求会返回 code:-352（已登录却提示 cookie 无效多由此而来，参考 DanmuFree 注释）
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(UA)
        .cookie_store(true)
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
    let mixin = crate::bilibili::wbi::mixin_key(&img_key, &sub_key);
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

/// WebSocket 单次会话：连接 → 认证 → 心跳 → 收消息，直到断开或取消
///
/// uid: 登录用户 UID（游客传 0，B 站不推送弹幕）
pub async fn run_ws_session(
    app: &tauri::AppHandle,
    room_id: u32,
    uid: i64,
    conf: DanmuConf,
    cancel: CancellationToken,
) -> Result<(), String> {
    // 依次尝试各个弹幕服务器
    let mut last_err = String::new();
    for host in &conf.hosts {
        match connect_once(app, room_id, uid, host, &conf.token, cancel.clone()).await {
            Ok(()) => return Ok(()), // 会话正常结束（被取消或主动断开）
            Err(e) => {
                last_err = e;
                if cancel.is_cancelled() {
                    return Ok(());
                }
            }
        }
    }
    Err(format!("所有弹幕服务器均连接失败: {last_err}"))
}

/// 连接单个服务器并持续收发，直到连接断开或被取消
async fn connect_once(
    app: &tauri::AppHandle,
    room_id: u32,
    uid: i64,
    host: &HostInfo,
    token: &str,
    cancel: CancellationToken,
) -> Result<(), String> {
    let url = format!("wss://{}:{}/sub", host.host, host.wss_port);
    let (mut ws, _resp) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|e| format!("WebSocket 连接 {url} 失败: {e}"))?;

    // 1. 发送认证包（uid 使用登录用户 UID，游客 0 无法收到弹幕）
    let auth = serde_json::json!({
        "uid": uid,
        "roomid": room_id,
        "protover": 3,
        "platform": "web",
        "type": 2,
        "key": token,
    })
    .to_string();
    ws.send(tokio_tungstenite::tungstenite::Message::Binary(
        protocol::encode_packet(OP_AUTH, auth.as_bytes()).into(),
    ))
    .await
    .map_err(|e| format!("发送认证包失败: {e}"))?;

    // 2. 心跳 + 读消息循环
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    heartbeat.tick().await; // 跳过首次立即触发

    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            _ = heartbeat.tick() => {
                ws.send(tokio_tungstenite::tungstenite::Message::Binary(
                    protocol::encode_packet(OP_HEARTBEAT, b"[object Object]").into(),
                )).await.map_err(|e| format!("发送心跳失败: {e}"))?;
            }
            msg = ws.next() => {
                match msg {
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(data))) => {
                        handle_frame(app, room_id, &data).map_err(|e| format!("帧解析失败: {e}"))?;
                    }
                    Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                        handle_frame(app, room_id, text.as_bytes())
                            .map_err(|e| format!("帧解析失败: {e}"))?;
                    }
                    Some(Ok(_)) => { /* Ping/Pong/Close 忽略 */ }
                    Some(Err(e)) => return Err(format!("WebSocket 读错误: {e}")),
                    None => return Err("WebSocket 连接已关闭".into()),
                }
            }
        }
    }
}

/// 处理一帧网络数据：拆包 → 展开(解压) → 解析 → 上报
fn handle_frame(app: &tauri::AppHandle, room_id: u32, data: &[u8]) -> Result<(), String> {
    let packets = protocol::decode_stream(data)?;
    for p in packets {
        match p.op {
            OP_AUTH_REPLY => {
                // 认证结果：body 为 {"code":0} 成功
                let code = serde_json::from_slice::<serde_json::Value>(&p.body)
                    .ok()
                    .and_then(|v| v.get("code").and_then(|c| c.as_i64()))
                    .unwrap_or(-1);
                if code != 0 {
                    return Err(format!("B 站认证失败 code={code}"));
                }
                // 同步内部状态机（供 get_connection_status / 轮询查询真实状态）
                crate::on_room_connected(app, room_id);
                // 认证成功：同步状态给前端
                let _ = app.emit(
                    "room-status",
                    serde_json::json!({ "state": "connected", "roomId": room_id }),
                );
            }
            OP_HEARTBEAT => { /* 心跳包忽略 */ }
            OP_MESSAGE => {
                for body in parser::expand_packet(&p)? {
                    for ev in parser::parse_payload(&body)? {
                        match ev {
                            BilibiliEvent::Danmaku(d) => {
                                let _ = app.emit("danmaku", d);
                            }
                            BilibiliEvent::Other(cmd) => {
                                eprintln!("[bilibili] 忽略命令: {cmd}");
                            }
                        }
                    }
                }
            }
            _ => { /* 其他操作码忽略 */ }
        }
    }
    Ok(())
}
