//! bili-danmu 核心入口（M2：B 站真实 WebSocket 弹幕连接 + 扫码登录）
//!
//! 连接流程：connect_room → 短号解析真实房间号 → 获取弹幕服务器配置(token)
//!   → WebSocket 认证(protover=3, 登录 UID) → 30s 心跳 → 实时接收弹幕 → emit 给前端
//! 登录：B 站 2025+ 要求登录态才推送弹幕，扫码登录后 cookie 持久化于本地配置

mod bilibili;
mod config;

use std::sync::Mutex;

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

/// 房间连接状态（内部维护，对外通过 room-status 事件同步）
#[derive(Clone, Copy, PartialEq)]
enum RoomStatus {
    Disconnected,
    Connecting { room_id: u32 },
    Connected { room_id: u32 },
}

struct AppState {
    status: Mutex<RoomStatus>,
    /// 当前连接任务的取消令牌（disconnect 时触发）
    cancel: Mutex<Option<CancellationToken>>,
    /// B 站登录态（None = 游客）
    auth: Mutex<Option<config::AuthInfo>>,
}

/// IPC 自检命令
#[tauri::command]
fn ping() -> String {
    "pong".into()
}

/// 查询登录态
#[tauri::command]
fn get_login_info(state: State<'_, AppState>) -> serde_json::Value {
    let auth = state.auth.lock().unwrap();
    json!({
        "loggedIn": auth.is_some(),
        "uid": auth.as_ref().map(|a| a.uid).unwrap_or(0),
    })
}

/// 生成 B 站登录二维码
#[tauri::command]
async fn qr_generate() -> Result<bilibili::login::QrData, String> {
    let client = bilibili::client::http_client();
    bilibili::login::qr_generate(&client).await
}

/// 轮询扫码结果；成功后持久化登录态并返回用户 UID
#[tauri::command]
async fn qr_poll(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> Result<bilibili::login::PollResult, String> {
    let client = bilibili::client::http_client();
    match bilibili::login::qr_poll(&client, &key).await? {
        bilibili::login::PollResult::Success { cookies } => {
            let uid = bilibili::login::cookie_value(&cookies, "DedeUserID")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            if uid == 0 {
                return Ok(bilibili::login::PollResult::Error {
                    message: "登录响应缺少 DedeUserID".into(),
                });
            }
            let auth = config::AuthInfo { uid, cookies };
            config::save_auth(&app, &auth).map_err(|e| e)?;
            *state.auth.lock().unwrap() = Some(auth);
            Ok(bilibili::login::PollResult::Success {
                cookies: uid.to_string(),
            })
        }
        other => Ok(other),
    }
}

/// 退出登录（清除本地登录态）
#[tauri::command]
fn logout(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    *state.auth.lock().unwrap() = None;
    config::clear_auth(&app)
}

/// 连接直播间：短号或真实房间号均可
#[tauri::command]
async fn connect_room(
    app: AppHandle,
    state: State<'_, AppState>,
    room_id: u32,
) -> Result<(), String> {
    {
        let mut st = state.status.lock().unwrap();
        match *st {
            RoomStatus::Disconnected => {}
            _ => return Err("已有连接，请先断开".into()),
        }
        *st = RoomStatus::Connecting { room_id };
    }
    let _ = app.emit("room-status", json!({ "state": "connecting", "roomId": room_id }));

    // 读取当前登录态（游客 uid=0 连上但收不到弹幕）
    let (uid, cookies) = {
        let auth = state.auth.lock().unwrap();
        match auth.as_ref() {
            Some(a) => (a.uid, a.cookies.clone()),
            None => (0, String::new()),
        }
    };

    let cancel = CancellationToken::new();
    *state.cancel.lock().unwrap() = Some(cancel.clone());

    spawn_connection(app.clone(), room_id, uid, cookies, cancel);
    Ok(())
}

/// 断开连接
#[tauri::command]
fn disconnect_room(app: AppHandle, state: State<'_, AppState>) {
    *state.status.lock().unwrap() = RoomStatus::Disconnected;
    if let Some(token) = state.cancel.lock().unwrap().take() {
        token.cancel();
    }
    let _ = app.emit("room-status", json!({ "state": "disconnected" }));
}

/// 完整连接流程：解析 → 鉴权 → WS 会话；结束后清理状态并上报结果
fn spawn_connection(
    app: AppHandle,
    short_id: u32,
    uid: i64,
    cookies: String,
    cancel: CancellationToken,
) {
    tauri::async_runtime::spawn(async move {
        let client = bilibili::client::http_client();

        let result: Result<(), String> = async {
            // 1. 短号解析真实房间号
            let resolved = bilibili::client::resolve_room(&client, short_id).await?;
            if resolved.live_status == 0 {
                eprintln!(
                    "[room] 房间 {} ({}) 当前未开播，仍尝试连接",
                    short_id, resolved.room_id
                );
            }
            // 2. 弹幕服务器配置（登录态优先；失败自动回退游客 token）
            let (conf, guest_mode) =
                bilibili::client::fetch_danmu_conf(&client, resolved.room_id, &cookies).await?;
            // 游客级 token 必须配 uid=0，否则服务器断开连接
            let auth_uid = if guest_mode { 0 } else { uid };
            // 3. WS 会话（认证成功后 client 会 emit connected；此处持续到断开/取消）
            bilibili::client::run_ws_session(&app, resolved.room_id, auth_uid, conf, cancel.clone())
                .await?;
            Ok(())
        }
        .await;

        let cancelled = cancel.is_cancelled();
        // 收尾：只有仍在进行中（未被手动断开）的状态才需要重置
        let st = app.state::<AppState>();
        let mut st = st.status.lock().unwrap();
        if *st != RoomStatus::Disconnected {
            *st = RoomStatus::Disconnected;
            drop(st);
            match (cancelled, result) {
                (true, _) | (_, Ok(())) => {
                    let _ = app.emit("room-status", json!({ "state": "disconnected" }));
                }
                (false, Err(msg)) => {
                    eprintln!("[bilibili] 连接结束: {msg}");
                    let _ = app.emit("room-status", json!({ "state": "error", "message": msg }));
                }
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            status: Mutex::new(RoomStatus::Disconnected),
            cancel: Mutex::new(None),
            auth: Mutex::new(None),
        })
        .setup(|app| {
            // 恢复本地登录态
            let loaded = config::load_auth(app.handle());
            if let Some(auth) = &loaded {
                let st = app.state::<AppState>();
                *st.auth.lock().unwrap() = Some(auth.clone());
                eprintln!("[auth] 已恢复登录态 uid={}", auth.uid);
            } else {
                eprintln!("[auth] 未登录（游客态收不到弹幕，请扫码登录）");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            connect_room,
            disconnect_room,
            get_login_info,
            qr_generate,
            qr_poll,
            logout,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
