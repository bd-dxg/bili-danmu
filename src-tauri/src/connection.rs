//! 直播间连接：命令入口 + 断线自动重连
//!
//! 连接流程：解析房间 → 弹幕会话（断线自动退避重连）→ 收尾清理状态并上报

use crate::bilibili;
use crate::config;
use crate::gift;
use crate::state::{clear_danmaku_ticks, AppState, RoomStatus};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio_util::sync::CancellationToken;

/// 连接直播间：短号或真实房间号均可
#[tauri::command]
pub(crate) async fn connect_room(
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

    let cancel = CancellationToken::new();
    *state.cancel.lock().unwrap() = Some(cancel.clone());

    // 登录态由 spawn_connection 每次会话前实时读取（重连期间用户可登录/退出）
    spawn_connection(app.clone(), room_id, cancel);
    Ok(())
}

/// 断开连接
#[tauri::command]
pub(crate) fn disconnect_room(app: AppHandle, state: State<'_, AppState>) {
    *state.status.lock().unwrap() = RoomStatus::Disconnected;
    clear_danmaku_ticks(&app);
    gift::clear(&app);
    if let Some(token) = state.cancel.lock().unwrap().take() {
        token.cancel();
    }
    let _ = app.emit("room-status", json!({ "state": "disconnected" }));
}

/// 查询当前连接状态（Overlay 轮询兑底，防事件丢失）
#[tauri::command]
pub(crate) fn get_connection_status(app: AppHandle) -> serde_json::Value {
    let st = app.state::<AppState>();
    let st = st.status.lock().unwrap();
    match *st {
        RoomStatus::Disconnected => json!({ "state": "disconnected" }),
        RoomStatus::Connecting { room_id } => {
            json!({ "state": "connecting", "roomId": room_id })
        }
        RoomStatus::Connected { room_id } => {
            json!({ "state": "connected", "roomId": room_id })
        }
    }
}

/// 后台记录最近连接的直播间：抓主播名 → 写配置 → 广播给主窗口刷新面包屑
///
/// 不放在连接流程里 await：主播名接口只影响面包屑显示，不该让弹幕会话等它。
fn spawn_recent_room(app: AppHandle, client: reqwest::Client, room_id: u32) {
    tauri::async_runtime::spawn(async move {
        // 抓不到主播名（接口失败）也只存房间号，前端回退显示房间号
        let uname = bilibili::api::fetch_anchor_uname(&client, room_id).await;
        let _ = config::save_recent_room(&app, &config::RecentRoom { room_id, uname });
        let _ = app.emit("recent-rooms-changed", ());
    });
}

/// 弹幕会话失败（断线/鉴权失败）自动重连的最大尝试次数；退避累计约 2.5 分钟后放弃并报错
const MAX_CONNECT_ATTEMPTS: u32 = 10;
/// 重连退避上限（秒），退避序列 1/2/4/…/封顶 30s
const RECONNECT_MAX_BACKOFF_SECS: u64 = 30;

/// 完整连接流程：解析房间 → 弹幕会话（断线自动退避重连）→ 收尾清理状态并上报
///
/// 房间解析失败不重试（房间不存在时重试无意义）；弹幕会话断开后按
/// 1/2/4/…/30s 退避静默自动重连（重连期间前端保持已连接语义，断流抖动自动恢复），
/// 超过 MAX_CONNECT_ATTEMPTS 次仍失败才报 error；用户可随时断开（取消令牌中止重试）。
fn spawn_connection(app: AppHandle, short_id: u32, cancel: CancellationToken) {
    tauri::async_runtime::spawn(async move {
        let client = bilibili::api::http_client();

        // 1. 短号解析真实房间号
        let resolved = match bilibili::api::resolve_room(&client, short_id).await {
            Ok(r) => r,
            Err(e) => {
                finish_connection(&app, &cancel, Err(e));
                return;
            }
        };
        if resolved.live_status == 0 {
            eprintln!(
                "[room] 房间 {} ({}) 当前未开播，仍尝试连接",
                short_id, resolved.room_id
            );
        }

        // 记录到最近房间（面包屑）：抓主播名是一次额外 HTTP，放在连接关键路径上会拖长
        // 「连接中…」（接口异常时最多 10s），故拆成后台任务；写完广播事件让主窗口刷新面包屑
        spawn_recent_room(app.clone(), client.clone(), resolved.room_id);

        // 2. 弹幕会话循环：断线自动重连，直到成功 / 用户断开 / 重试耗尽
        let mut attempt = 0u32;
        let result: Result<(), String> = loop {
            if attempt > 0 {
                // 退避等待（可取消）
                let backoff = (1u64 << attempt.saturating_sub(1).min(5))
                    .min(RECONNECT_MAX_BACKOFF_SECS);
                eprintln!("[bilibili] 会话断开，{backoff}s 后重连（第 {attempt} 次）");
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(backoff)) => {}
                    _ = cancel.cancelled() => break Ok(()),
                }
            }
            if cancel.is_cancelled() {
                break Ok(());
            }

            // 每次会话前实时读取登录态（游客 uid=0 连上但收不到弹幕）
            let (uid, cookies) = {
                let st = app.state::<AppState>();
                let auth = st.auth.lock().unwrap();
                match auth.as_ref() {
                    Some(a) => (a.uid, a.cookies.clone()),
                    None => (0, String::new()),
                }
            };

            // 弹幕服务器配置（登录态优先；失败自动回退游客 token）
            let (conf, guest_mode) =
                match bilibili::api::fetch_danmu_conf(&client, resolved.room_id, &cookies).await
                {
                    Ok(v) => v,
                    Err(e) => {
                        attempt += 1;
                        if attempt >= MAX_CONNECT_ATTEMPTS {
                            break Err(e);
                        }
                        continue;
                    }
                };
            // 游客级 token 必须配 uid=0，否则服务器断开连接
            let auth_uid = if guest_mode { 0 } else { uid };

            // WS 会话（认证成功后 client 会 emit connected）；断开返回 Err 进入重连
            match bilibili::ws::run_ws_session(
                &app,
                resolved.room_id,
                auth_uid,
                conf,
                cancel.clone(),
            )
            .await
            {
                Ok(()) => break Ok(()), // 被取消，正常结束
                Err(e) => {
                    attempt += 1;
                    if attempt >= MAX_CONNECT_ATTEMPTS {
                        break Err(e);
                    }
                }
            }
        };

        finish_connection(&app, &cancel, result);
    });
}

/// 连接任务收尾：仅当自己仍是当前连接任务（取消令牌未被换走）时重置状态并上报
///
/// disconnect_room 会 take 令牌、新连接会替换令牌——若收尾时状态里存的令牌
/// 已不是自己，说明已被断开或已有新连接接管，绝不能重置状态，否则会把新连接的
/// Connected 误清为 Disconnected（「断开 → 立即重连」时序下的旧任务竞态）。
fn finish_connection(app: &AppHandle, cancel: &CancellationToken, result: Result<(), String>) {
    let st = app.state::<AppState>();
    let is_current = st.cancel.lock().unwrap().as_ref() == Some(cancel);
    if !is_current {
        return;
    }
    *st.status.lock().unwrap() = RoomStatus::Disconnected;
    clear_danmaku_ticks(app);
    gift::clear(app);
    match result {
        Ok(()) => {
            let _ = app.emit("room-status", json!({ "state": "disconnected" }));
        }
        Err(msg) => {
            eprintln!("[bilibili] 连接结束: {msg}");
            let _ = app.emit("room-status", json!({ "state": "error", "message": msg }));
        }
    }
}
