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
    /// 显示窗口边界参考线（独立开关，方便拖拽/调整）
    boundary: Mutex<bool>,
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
            boundary: Mutex::new(false),
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
    // 广播给 Overlay 页面：穿透关闭时显示窗口边界虚线，方便拖拽/调整
    let _ = app.emit("overlay-clickthrough", enabled);
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
    config::save_overlay_style(&app, &style)
}

/// 开关显示边界参考线（广播给 Overlay 页面）
#[tauri::command]
fn overlay_set_boundary(
    app: AppHandle,
    state: State<'_, OverlayState>,
    show: bool,
) -> Result<bool, String> {
    *state.boundary.lock().unwrap() = show;
    let _ = app.emit("overlay-boundary", show);
    Ok(show)
}

/// 查询边界参考线状态
#[tauri::command]
fn overlay_get_boundary(state: State<'_, OverlayState>) -> bool {
    *state.boundary.lock().unwrap()
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
            }

            // Overlay：透明 / 无边框 / 置顶 / 可调整 / 跳过任务栏，恢复上次位置
            let mut win_builder = WebviewWindowBuilder::new(
                app,
                "overlay",
                WebviewUrl::App("overlay.html".into()),
            )
            .title("bili-danmu overlay")
            .inner_size(480.0, 240.0)
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
            overlay_set_visible,
            overlay_set_clickthrough,
            overlay_get_clickthrough,
            overlay_is_visible,
            overlay_set_always_on_top,
            overlay_get_style,
            overlay_set_style,
            overlay_set_boundary,
            overlay_get_boundary,
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