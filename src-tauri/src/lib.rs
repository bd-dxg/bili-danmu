//! bili-danmu 核心入口（M1：脚手架 + IPC 链路验证）
//!
//! M1 的 connect_room 为模拟流程：
//! 收到连接请求 → emit connecting → 400ms 后 emit connected → 每秒 emit 心跳。
//! 心跳事件用于验证「Rust 后台任务 → Vue 前端」事件推送链路，
//! M2 将用真实 B 站 WebSocket 连接替换此流程。

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};

/// 房间连接状态（内部维护，对外通过 room-status 事件同步）
enum RoomStatus {
    Disconnected,
    Connecting { room_id: u32 },
    Connected { room_id: u32 },
}

struct AppState {
    status: Mutex<RoomStatus>,
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// IPC 自检命令
#[tauri::command]
fn ping() -> String {
    "pong".into()
}

/// 连接直播间（M1 模拟，M2 替换为真实连接）
#[tauri::command]
async fn connect_room(
    app: AppHandle,
    state: State<'_, AppState>,
    room_id: u32,
) -> Result<(), String> {
    {
        let mut st = state.status.lock().unwrap();
        match &*st {
            RoomStatus::Disconnected => {}
            _ => return Err("已有连接，请先断开".into()),
        }
        *st = RoomStatus::Connecting { room_id };
    }

    let _ = app.emit(
        "room-status",
        json!({ "state": "connecting", "roomId": room_id }),
    );

    spawn_connect_flow(app.clone(), room_id);
    Ok(())
}

/// 断开连接
#[tauri::command]
fn disconnect_room(app: AppHandle, state: State<'_, AppState>) {
    *state.status.lock().unwrap() = RoomStatus::Disconnected;
    let _ = app.emit("room-status", json!({ "state": "disconnected" }));
}

/// 连接流程：模拟握手 → connected → 心跳循环
/// 心跳循环每轮校验状态，断开后自动退出
fn spawn_connect_flow(app: AppHandle, room_id: u32) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;

        // 状态仍为 Connecting 时才升级为 Connected（防止期间已断开）
        {
            let st = app.state::<AppState>();
            let mut g = st.status.lock().unwrap();
            let still_connecting = match &*g {
                RoomStatus::Connecting { room_id: r } => *r == room_id,
                _ => false,
            };
            if !still_connecting {
                return;
            }
            *g = RoomStatus::Connected { room_id };
        }
        let _ = app.emit(
            "room-status",
            json!({ "state": "connected", "roomId": room_id }),
        );

        // 心跳循环（M1 用，验证后台 emit）
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            let active = {
                let st = app.state::<AppState>();
                let g = st.status.lock().unwrap();
                match &*g {
                    RoomStatus::Connected { room_id: r } => *r == room_id,
                    _ => false,
                }
            };
            if !active {
                break;
            }
            let _ = app.emit("heartbeat", json!({ "ts": now_ts() }));
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            status: Mutex::new(RoomStatus::Disconnected),
        })
        .invoke_handler(tauri::generate_handler![ping, connect_room, disconnect_room])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
