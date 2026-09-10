//! B 站弹幕 WebSocket 会话：连接 → 认证 → 心跳 → 收消息
//!
//! HTTP 侧的鉴权与配置获取见 `api.rs`。
//!
//! 流程：wss 连接 → 认证包(protover=3) → 30s 心跳 → 收弹幕事件

use crate::bilibili::api::{DanmuConf, HostInfo};
use crate::bilibili::event::BilibiliEvent;
use crate::bilibili::parser;
use crate::bilibili::protocol::{self, OP_AUTH, OP_AUTH_REPLY, OP_HEARTBEAT, OP_MESSAGE};
use futures_util::{SinkExt, StreamExt};
use std::time::Duration;
use tauri::Emitter;
use tokio_util::sync::CancellationToken;

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
    // 连接带 10s 超时：服务器不可达时快速失败，避免 run_ws_session 串行遍历
    // host 列表时每个挂起等系统超时（最坏累计分钟级无反馈）
    let (mut ws, _resp) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async(&url),
    )
    .await
    .map_err(|_| format!("WebSocket 连接 {url} 超时"))?
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
                        // 单变体：所有事件都是弹幕
                        let BilibiliEvent::Danmaku(d) = ev;
                        // 朗读是与显示并列的独立消费者（关闭朗读不影响弹幕窗）
                        crate::record_danmaku(app);
                        crate::tts::on_danmaku(app, &d);
                        let _ = app.emit("danmaku", d);
                    }
                }
            }
            _ => { /* 其他操作码忽略 */ }
        }
    }
    Ok(())
}
