//! bili-danmu 核心入口（M2：B 站真实 WebSocket 弹幕连接 + 扫码登录）
//!
//! 连接流程：connect_room → 短号解析真实房间号 → 获取弹幕服务器配置(token)
//!   → WebSocket 认证(protover=3, 登录 UID) → 30s 心跳 → 实时接收弹幕 → emit 给前端
//! 登录：B 站 2025+ 要求登录态才推送弹幕，扫码登录后 cookie 持久化于本地配置
//!
//! 模块分工：状态见 `state.rs`，IPC 命令见 `commands.rs`，连接与重连见 `connection.rs`，
//! 窗口辅助见 `window.rs`，本文件只做组装（状态托管、窗口创建、托盘、命令注册）。

mod bilibili;
mod commands;
mod config;
mod connection;
mod gift;
mod state;
mod tts;
mod window;

use state::{AppState, OverlayState};
use std::collections::VecDeque;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use window::{capture_overlay_bounds, flush_overlay_bounds, sync_sender_docked};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(AppState {
            status: Mutex::new(state::RoomStatus::Disconnected),
            cancel: Mutex::new(None),
            auth: Mutex::new(None),
            danmaku_ticks: Mutex::new(VecDeque::new()),
        })
        .manage(OverlayState::default())
        .manage(tts::TtsState::new(config::TtsConfig::default()))
        .manage(gift::GiftState::default())
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
                *ov.gift.lock().unwrap() = cfg.gift.clone();
            }
            // TTS：恢复朗读配置并启动串行朗读 worker（与弹幕显示完全解耦）
            app.state::<tts::TtsState>()
                .set_config(cfg.tts.clone());
            if cfg.tts.enabled {
                eprintln!("[tts] 已恢复朗读开关，音色={}", cfg.tts.voice);
            }
            tts::spawn_worker(app.handle().clone());

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
            commands::ping,
            connection::connect_room,
            connection::disconnect_room,
            connection::get_connection_status,
            commands::get_login_info,
            commands::qr_generate,
            commands::qr_poll,
            commands::logout,
            commands::send_danmaku,
            commands::overlay_set_visible,
            commands::overlay_set_clickthrough,
            commands::overlay_get_clickthrough,
            commands::overlay_is_visible,
            commands::overlay_set_always_on_top,
            commands::get_dashboard_status,
            commands::overlay_get_style,
            commands::overlay_set_style,
            commands::danmaku_get_filter,
            commands::danmaku_set_filter,
            commands::gift_get_config,
            commands::gift_set_config,
            commands::tts_get_config,
            commands::tts_set_config,
            commands::tts_test_speak,
            commands::tts_list_voices,
            commands::tts_refresh_voices,
            commands::get_recent_rooms,
            commands::overlay_get_size,
            commands::overlay_set_size,
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
