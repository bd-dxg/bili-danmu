//! Overlay / Sender 窗口辅助：取窗口句柄、吸附布局、位置落盘
//!
//! 窗口的创建与事件绑定在 `lib.rs` 的 setup 里，这里只放被多处复用的操作。

use crate::config;
use crate::state::OverlayState;
use tauri::{AppHandle, Emitter, Manager};

/// 获取 Overlay 窗口实例（不存在返回 None）
pub(crate) fn overlay_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window("overlay")
}

/// 获取 Sender 窗口实例（不存在返回 None）
pub(crate) fn sender_window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window("sender")
}

/// 读取窗口当前矩形 → 更新内存态；尺寸变化时广播给主界面「弹幕窗」标签的宽高滑块
///
/// 统一换算成**逻辑像素**再存：窗口 API（outer_position / inner_size）返回物理像素，
/// 高 DPI 下差一个 scale_factor 的倍数；而恢复窗口用的 position/inner_size(构建器)
/// 与设置页的 px 都是逻辑像素，不换算就会出现「存的比恢复的大」。
pub(crate) fn capture_overlay_bounds(app: &AppHandle) {
    let Some(win) = overlay_window(app) else { return };
    let Ok(pos) = win.outer_position() else { return };
    let Ok(size) = win.inner_size() else { return };
    let scale = win.scale_factor().unwrap_or(1.0);
    let pos = pos.to_logical::<f64>(scale);
    let size = size.to_logical::<f64>(scale);
    let b = config::WindowBounds {
        x: pos.x,
        y: pos.y,
        w: size.width,
        h: size.height,
    };
    let st = app.state::<OverlayState>();
    let size_changed = {
        let mut bounds = st.bounds.lock().unwrap();
        let changed = bounds.map(|p| (p.w, p.h) != (b.w, b.h)).unwrap_or(true);
        *bounds = Some(b);
        changed
    };
    // 只在尺寸变化时广播：拖动窗口（只改位置）会每帧触发窗口事件，全发会把 IPC 刷满。
    // 取整后发出，与 overlay_get_size 同一口径（滑块的取值是整数）。
    if size_changed {
        let _ = app.emit(
            "overlay-size",
            serde_json::json!({ "width": size.width.round(), "height": size.height.round() }),
        );
    }
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
/// 缩进 = 容器 padding 6px + role-slot 4.8em + 0.7em 间距，
/// 与 OverlayApp.vue 的 --role-indent 一致（改一处要同步改另一处）。
fn sender_layout_metrics(app: &AppHandle, scale: f64) -> (u32, u32, u32) {
    let font_size = app.state::<OverlayState>().style.lock().unwrap().font_size;
    let indent = ((6.0 + 5.5 * font_size) * scale).round() as u32;
    let right_pad = (6.0 * scale).round() as u32;
    let h = ((font_size * 3.8 + 8.0) * scale).round() as u32;
    (indent, right_pad, h)
}
