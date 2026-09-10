//! bili-danmu 核心入口（M2：B 站真实 WebSocket 弹幕连接 + 扫码登录）
//!
//! 连接流程：connect_room → 短号解析真实房间号 → 获取弹幕服务器配置(token)
//!   → WebSocket 认证(protover=3, 登录 UID) → 30s 心跳 → 实时接收弹幕 → emit 给前端
//! 登录：B 站 2025+ 要求登录态才推送弹幕，扫码登录后 cookie 持久化于本地配置

mod bilibili;
mod config;

use std::sync::Mutex;

use serde_json::json;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tokio_util::sync::CancellationToken;

/// 房间连接状态（内部维护，对外通过 room-status 事件同步）
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum RoomStatus {
    Disconnected,
    Connecting { room_id: u32 },
    Connected { room_id: u32 },
}

/// B 站 WebSocket 认证成功后由 client 回调，同步内部状态机
pub(crate) fn on_room_connected(app: &AppHandle, room_id: u32) {
    let st = app.state::<AppState>();
    *st.status.lock().unwrap() = RoomStatus::Connected { room_id };
}

struct AppState {
    status: Mutex<RoomStatus>,
    /// 当前连接任务的取消令牌（disconnect 时触发）
    cancel: Mutex<Option<CancellationToken>>,
    /// B 站登录态（None = 游客）
    auth: Mutex<Option<config::AuthInfo>>,
}

/// Overlay 窗口状态（供 UI 同步开关与弹幕样式）
struct OverlayState {
    clickthrough: Mutex<bool>,
    style: Mutex<config::OverlayStyle>,
    /// 弹幕过滤配置（内存态，供 Overlay 实时读取；变更广播 danmaku-filter 事件）
    filter: Mutex<config::DanmakuFilter>,
    /// 窗口当前位置大小（内存态，由 flush 周期/退出时落盘）
    bounds: Mutex<Option<config::WindowBounds>>,
    /// 已落盘的位置大小（去重，避免无变化时反复写盘）
    saved_bounds: Mutex<Option<config::WindowBounds>>,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            clickthrough: Mutex::new(false),
            style: Mutex::new(config::OverlayStyle::default()),
            filter: Mutex::new(config::DanmakuFilter::default()),
            bounds: Mutex::new(None),
            saved_bounds: Mutex::new(None),
        }
    }
}

/// 读取窗口当前矩形 → 更新内存态
fn capture_overlay_bounds(app: &AppHandle) {
    let Some(win) = overlay_window(app) else { return };
    let Ok(pos) = win.outer_position() else { return };
    let Ok(size) = win.inner_size() else { return };
    let b = config::WindowBounds {
        x: pos.x as f64,
        y: pos.y as f64,
        w: size.width as f64,
        h: size.height as f64,
    };
    let st = app.state::<OverlayState>();
    *st.bounds.lock().unwrap() = Some(b);
}

/// 内存态位置与已保存不同则写盘（去重）
fn flush_overlay_bounds(app: &AppHandle) {
    let st = app.state::<OverlayState>();
    let current = *st.bounds.lock().unwrap();
    if current.is_none() {
        return;
    }
    let saved = *st.saved_bounds.lock().unwrap();
    if current != saved {
        let _ = config::save_overlay_bounds(app, &current.unwrap());
        *st.saved_bounds.lock().unwrap() = current;
    }
}

/// 获取 Overlay 窗口实例（不存在返回 None）
fn overlay_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window("overlay")
}

/// 获取 Sender 窗口实例（不存在返回 None）
fn sender_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window("sender")
}

/// 发送框始终吸附：对齐到弹幕窗正文列下方（左端缩进 role-slot，宽度跟随，高度随字号）
fn sync_sender_docked(app: &AppHandle) {
    let (Some(ov), Some(sender)) = (overlay_window(app), sender_window(app)) else {
        return;
    };
    let Ok(pos) = ov.outer_position() else { return };
    let Ok(size) = ov.outer_size() else { return };
    let scale = ov.scale_factor().unwrap_or(1.0);
    let (indent, right_pad, h) = sender_layout_metrics(app, scale);
    let x = pos.x + indent as i32;
    let y = pos.y + size.height as i32;
    let w = size.width.saturating_sub(indent + right_pad).max(1);
    let _ = sender.set_position(tauri::Position::Physical(tauri::PhysicalPosition::new(x, y)));
    let _ = sender.set_size(tauri::Size::Physical(tauri::PhysicalSize::new(w, h)));
}

/// 发送框相对弹幕窗的缩进/右距/高度（物理 px），随弹幕字号缩放。
/// 缩进 = 容器 padding 6px + role-slot 4.8em + 0.35em 间距，与 OverlayApp.vue 正文列起点一致。
fn sender_layout_metrics(app: &AppHandle, scale: f64) -> (u32, u32, u32) {
    let font_size = app.state::<OverlayState>().style.lock().unwrap().font_size;
    let indent = ((6.0 + 5.15 * font_size) * scale).round() as u32;
    let right_pad = (6.0 * scale).round() as u32;
    let h = ((font_size * 3.8 + 8.0) * scale).round() as u32;
    (indent, right_pad, h)
}

/// 显示 / 隐藏 Overlay（返回新可见状态）
#[tauri::command]
fn overlay_set_visible(app: AppHandle, visible: bool) -> Result<bool, String> {
    let win = overlay_window(&app).ok_or("Overlay 窗口未创建")?;
    if visible {
        win.show().map_err(|e| format!("显示失败: {e}"))?;
    } else {
        win.hide().map_err(|e| format!("隐藏失败: {e}"))?;
    }
    Ok(visible)
}

/// 开关鼠标穿透（核心桌面功能：开启后鼠标事件穿透 Overlay 落到下层窗口）
#[tauri::command]
fn overlay_set_clickthrough(
    app: AppHandle,
    state: State<'_, OverlayState>,
    enabled: bool,
) -> Result<bool, String> {
    let win = overlay_window(&app).ok_or("Overlay 窗口未创建")?;
    win.set_ignore_cursor_events(enabled)
        .map_err(|e| format!("设置穿透失败: {e}"))?;
    *state.clickthrough.lock().unwrap() = enabled;
    Ok(enabled)
}

/// 查询当前穿透状态
#[tauri::command]
fn overlay_get_clickthrough(state: State<'_, OverlayState>) -> bool {
    *state.clickthrough.lock().unwrap()
}

/// 查询 Overlay 是否可见
#[tauri::command]
fn overlay_is_visible(app: AppHandle) -> bool {
    overlay_window(&app)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// 开关始终置顶
#[tauri::command]
fn overlay_set_always_on_top(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let win = overlay_window(&app).ok_or("Overlay 窗口未创建")?;
    win.set_always_on_top(enabled)
        .map_err(|e| format!("设置置顶失败: {e}"))?;
    Ok(enabled)
}
/// 读取当前 Overlay 弹幕样式
#[tauri::command]
fn overlay_get_style(state: State<'_, OverlayState>) -> config::OverlayStyle {
    state.style.lock().unwrap().clone()
}

/// 更新 Overlay 弹幕样式：广播给 Overlay 窗口并持久化
#[tauri::command]
fn overlay_set_style(
    app: AppHandle,
    state: State<'_, OverlayState>,
    style: config::OverlayStyle,
) -> Result<(), String> {
    *state.style.lock().unwrap() = style.clone();
    let _ = app.emit("overlay-style", &style);
    config::save_overlay_style(&app, &style)?;
    // 弹幕字号变化影响发送框缩进与高度，同步重对齐
    sync_sender_docked(&app);
    Ok(())
}

/// 读取当前弹幕过滤配置
#[tauri::command]
fn danmaku_get_filter(state: State<'_, OverlayState>) -> config::DanmakuFilter {
    state.filter.lock().unwrap().clone()
}

/// 更新弹幕过滤配置：广播给 Overlay 窗口（实时生效）并持久化
#[tauri::command]
fn danmaku_set_filter(
    app: AppHandle,
    state: State<'_, OverlayState>,
    filter: config::DanmakuFilter,
) -> Result<(), String> {
    *state.filter.lock().unwrap() = filter.clone();
    let _ = app.emit("danmaku-filter", &filter);
    config::save_danmaku_filter(&app, &filter)
}

/// 查询 Overlay 当前尺寸（逻辑像素）
#[tauri::command]
fn overlay_get_size(app: AppHandle) -> Result<serde_json::Value, String> {
    let win = overlay_window(&app).ok_or("Overlay 窗口未创建")?;
    let size = win.inner_size().map_err(|e| format!("读取尺寸失败: {e}"))?;
    Ok(json!({ "width": size.width, "height": size.height }))
}

/// 调整 Overlay 宽高（设置页滑块调用；尺寸变化自动被窗口事件捕获落盘）
#[tauri::command]
fn overlay_set_size(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    let win = overlay_window(&app).ok_or("Overlay 窗口未创建")?;
    win.set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| format!("调整尺寸失败: {e}"))
}

/// 查询当前连接状态（Overlay 轮询兑底，防事件丢失）
#[tauri::command]
fn get_connection_status(app: AppHandle) -> serde_json::Value {
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
        "uname": auth.as_ref().and_then(|a| a.uname.clone()),
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
            // 顺带拉取昵称（失败不影响登录，仅显示用）
            let uname = bilibili::login::fetch_uname(&client, &cookies).await.ok();
            let auth = config::AuthInfo {
                uid,
                cookies: cookies.clone(),
                uname,
            };
            config::save_auth(&app, &auth)?;
            *state.auth.lock().unwrap() = Some(auth);
            // 原样回传登录 Cookie 串（前端仅消费 status 字段，不需要关心内容）——
            // 不复用 cookies 字段传 uid，保持与 login::PollResult 的字段语义一致
            Ok(bilibili::login::PollResult::Success { cookies })
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

/// 发送弹幕到当前连接的直播间（需登录态）
#[tauri::command]
async fn send_danmaku(state: State<'_, AppState>, msg: String) -> Result<(), String> {
    let msg = msg.trim();
    if msg.is_empty() {
        return Err("弹幕内容不能为空".into());
    }
    if msg.chars().count() > 100 {
        return Err("弹幕最长 100 字".into());
    }

    // 登录态（含 bili_jct）——锁作用域内克隆，避免持锁 await
    let cookies = {
        let auth = state.auth.lock().unwrap();
        match auth.as_ref() {
            Some(a) => a.cookies.clone(),
            None => return Err("请先扫码登录后再发送弹幕".into()),
        }
    };

    // 当前连接的真实房间号（未连接时无发送目标）
    let room_id = {
        let st = state.status.lock().unwrap();
        match *st {
            RoomStatus::Connected { room_id } => room_id,
            _ => return Err("请先连接直播间后再发送弹幕".into()),
        }
    };

    let client = bilibili::client::http_client();
    bilibili::send::send_danmaku(&client, &cookies, room_id, msg).await
}

/// 查询最近连接的直播间（主界面面包屑，已按最近在前排序）
#[tauri::command]
fn get_recent_rooms(app: AppHandle) -> Vec<config::RecentRoom> {
    config::load_config(&app).recent_rooms
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

    let cancel = CancellationToken::new();
    *state.cancel.lock().unwrap() = Some(cancel.clone());

    // 登录态由 spawn_connection 每次会话前实时读取（重连期间用户可登录/退出）
    spawn_connection(app.clone(), room_id, cancel);
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
        let client = bilibili::client::http_client();

        // 1. 短号解析真实房间号
        let resolved = match bilibili::client::resolve_room(&client, short_id).await {
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

        // 记录到最近房间（面包屑）：主播名请求失败不阻断连接，回退只存房间号
        let uname = bilibili::client::fetch_anchor_uname(&client, resolved.room_id).await;
        let _ = config::save_recent_room(
            &app,
            &config::RecentRoom {
                room_id: resolved.room_id,
                uname,
            },
        );

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
                match bilibili::client::fetch_danmu_conf(&client, resolved.room_id, &cookies).await
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
            match bilibili::client::run_ws_session(
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(AppState {
            status: Mutex::new(RoomStatus::Disconnected),
            cancel: Mutex::new(None),
            auth: Mutex::new(None),
        })
        .manage(OverlayState::default())
        .setup(|app| {
            // 加载持久化配置：登录态 → AppState.auth；样式/位置 → OverlayState
            let cfg = config::load_config(app.handle());
            if let Some(auth) = &cfg.auth {
                let st = app.state::<AppState>();
                *st.auth.lock().unwrap() = Some(auth.clone());
                eprintln!("[auth] 已恢复登录态 uid={}", auth.uid);
            } else {
                eprintln!("[auth] 未登录（游客态收不到弹幕，请扫码登录）");
            }
            {
                let ov = app.state::<OverlayState>();
                *ov.style.lock().unwrap() = cfg.overlay_style.clone();
                *ov.filter.lock().unwrap() = cfg.danmaku_filter.clone();
            }

            // Overlay：透明 / 无边框 / 置顶 / 可调整 / 跳过任务栏，恢复上次位置
            let mut win_builder = WebviewWindowBuilder::new(
                app,
                "overlay",
                WebviewUrl::App("overlay.html".into()),
            )
            .title("bili-danmu overlay")
            .inner_size(700.0, 400.0)
            .position(80.0, 80.0)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .resizable(true)
            .min_inner_size(120.0, 40.0)
            .skip_taskbar(true)
            .shadow(false);
            if let Some(b) = cfg.overlay_bounds {
                win_builder = win_builder.position(b.x, b.y).inner_size(b.w, b.h);
            }
            if let Ok(win) = win_builder.build() {
                let app2 = app.handle().clone();
                win.on_window_event(move |event| {
                    if matches!(
                        event,
                        tauri::WindowEvent::Moved(_) | tauri::WindowEvent::Resized(_)
                    ) {
                        capture_overlay_bounds(&app2);
                        // 发送框始终吸附：弹幕窗移动/缩放时跟随
                        sync_sender_docked(&app2);
                    }
                });
            } else {
                eprintln!("[overlay] 创建失败");
            }
            // 周期落盘窗口位置（拖动/缩放中去重保存）
            let app3 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let mut interval =
                    tokio::time::interval(std::time::Duration::from_secs(2));
                loop {
                    interval.tick().await;
                    flush_overlay_bounds(&app3);
                }
            });

            // Sender：发送框始终吸附在弹幕窗下方（无边框 / 置顶 / 跳过任务栏）
            let sender_builder = WebviewWindowBuilder::new(
                app,
                "sender",
                WebviewUrl::App("sender.html".into()),
            )
            .title("bili-danmu 发送")
            .inner_size(320.0, 80.0)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .resizable(false)
            .skip_taskbar(true)
            .shadow(false);
            if sender_builder.build().is_err() {
                eprintln!("[sender] 创建失败");
            }
            // 创建后立即对齐到弹幕窗下方（覆盖默认位置/尺寸）
            sync_sender_docked(app.handle());

            // 主窗口 × → 最小化到托盘（不退出；由托盘菜单唤出/退出）
            if let Some(main_win) = app.get_webview_window("main") {
                let app2 = app.handle().clone();
                main_win.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = app2.get_webview_window("main").map(|w| w.hide());
                    }
                });
            }

            // 系统托盘：左键单击显示设置窗口
            let show_item = MenuItem::with_id(app, "show", "显示设置", true, None::<&str>)
                .map_err(|e| eprintln!("[tray] 创建菜单项失败: {e}"))
                .ok();
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)
                .map_err(|e| eprintln!("[tray] 创建菜单项失败: {e}"))
                .ok();
            let mut items: Vec<&dyn tauri::menu::IsMenuItem<_>> = Vec::new();
            if let Some(i) = &show_item {
                items.push(i);
            }
            if let Some(i) = &quit_item {
                items.push(i);
            }
            if let Ok(menu) = Menu::with_items(app, &items) {
                let tray = TrayIconBuilder::with_id("main-tray")
                    .icon(app.default_window_icon().expect("default window icon").clone())
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id().as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                        "quit" => {
                            // 先销毁窗口再退出：destroy 绕过主窗 ×→隐藏 拦截，
                            // 让 WebView2 在进程退出前释放 HWND，避免 1412 unregister 竞态
                            let wins: Vec<_> =
                                app.webview_windows().into_values().collect();
                            for w in wins {
                                let _ = w.destroy();
                            }
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            let app = tray.app_handle();
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                    })
                    .build(app)
                    .map_err(|e| eprintln!("[tray] 创建托盘失败: {e}"))
                    .ok();
                let _ = tray; // 托盘由 tauri 管理，持有即保活
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
            send_danmaku,
            overlay_set_visible,
            overlay_set_clickthrough,
            overlay_get_clickthrough,
            overlay_is_visible,
            overlay_set_always_on_top,
            overlay_get_style,
            overlay_set_style,
            danmaku_get_filter,
            danmaku_set_filter,
            get_recent_rooms,
            overlay_get_size,
            overlay_set_size,
            get_connection_status,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // 退出前落盘 Overlay 窗口位置
    app.run(|handle, event| {
        if let tauri::RunEvent::Exit = event {
            flush_overlay_bounds(handle);
        }
    });
}