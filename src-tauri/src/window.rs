//! Overlay / Sender 窗口辅助：取窗口句柄、吸附布局、位置落盘
//!
//! 窗口的创建与事件绑定在 `lib.rs` 的 setup 里，这里只放被多处复用的操作。

use crate::config;
use crate::state::OverlayState;
use tauri::{AppHandle, Manager};

/// 获取 Overlay 窗口实例（不存在返回 None）
pub(crate) fn overlay_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window("overlay")
}

/// 获取 Sender 窗口实例（不存在返回 None）
pub(crate) fn sender_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window("sender")
}

/// 读取窗口当前矩形 → 更新内存态
pub(crate) fn capture_overlay_bounds(app: &AppHandle) {
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
pub(crate) fn flush_overlay_bounds(app: &AppHandle) {
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

/// 发送框始终吸附：对齐到弹幕窗正文列下方（左端缩进 role-slot，宽度跟随，高度随字号）
pub(crate) fn sync_sender_docked(app: &AppHandle) {
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
