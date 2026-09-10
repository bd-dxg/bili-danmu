//! 应用级内存状态（Tauri 托管）
//!
//! 三块互不相关的状态放在一起，因为它们的读写方都是 IPC 命令与窗口事件：
//! - `RoomStatus`：直播间连接状态机（对外通过 room-status 事件同步）
//! - 弹幕速率窗口：仪表盘「最近 RATE_WINDOW_SECS 秒接收条数」
//! - `OverlayState`：弹幕窗开关、样式、过滤与位置大小的内存态

use crate::config;
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;

/// 房间连接状态（内部维护，对外通过 room-status 事件同步）
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum RoomStatus {
    Disconnected,
    Connecting { room_id: u32 },
    Connected { room_id: u32 },
}

pub(crate) struct AppState {
    pub(crate) status: Mutex<RoomStatus>,
    /// 当前连接任务的取消令牌（disconnect 时触发）
    pub(crate) cancel: Mutex<Option<CancellationToken>>,
    /// B 站登录态（None = 游客）
    pub(crate) auth: Mutex<Option<config::AuthInfo>>,
    /// 最近接收弹幕的时刻（滑动窗口，供仪表盘算「条/10 秒」）
    pub(crate) danmaku_ticks: Mutex<VecDeque<Instant>>,
}

/// B 站 WebSocket 认证成功后由 client 回调，同步内部状态机
pub(crate) fn on_room_connected(app: &AppHandle, room_id: u32) {
    let st = app.state::<AppState>();
    *st.status.lock().unwrap() = RoomStatus::Connected { room_id };
}

/// 弹幕速率统计窗口（秒）：仪表盘展示「最近 RATE_WINDOW_SECS 秒接收条数」
const RATE_WINDOW_SECS: u64 = 10;

/// 记录一条已接收弹幕（速率统计，在弹幕事件回调里调用，非阻塞）
pub(crate) fn record_danmaku(app: &AppHandle) {
    let now = Instant::now();
    let st = app.state::<AppState>();
    let mut ticks = st.danmaku_ticks.lock().unwrap();
    ticks.push_back(now);
    prune_ticks(&mut ticks, now);
}

/// 丢弃滑动窗口外的时间戳（时间戳单调递增，只看队首即可）
pub(crate) fn prune_ticks(ticks: &mut VecDeque<Instant>, now: Instant) {
    while let Some(front) = ticks.front() {
        if now.duration_since(*front).as_secs() >= RATE_WINDOW_SECS {
            ticks.pop_front();
        } else {
            break;
        }
    }
}

/// 清空速率窗口（断开连接时调用，避免仪表盘残留断开前的旧速率）
pub(crate) fn clear_danmaku_ticks(app: &AppHandle) {
    app.state::<AppState>().danmaku_ticks.lock().unwrap().clear();
}

/// Overlay 窗口状态（供 UI 同步开关与弹幕样式）
pub(crate) struct OverlayState {
    pub(crate) clickthrough: Mutex<bool>,
    /// 始终置顶（窗口 API 无 getter，需内存态供仪表盘读取；初值与创建窗口时一致）
    pub(crate) always_on_top: Mutex<bool>,
    pub(crate) style: Mutex<config::OverlayStyle>,
    /// 弹幕过滤配置（内存态，供 Overlay 实时读取；变更广播 danmaku-filter 事件）
    pub(crate) filter: Mutex<config::DanmakuFilter>,
    /// 窗口当前位置大小（内存态，由 flush 周期/退出时落盘）
    pub(crate) bounds: Mutex<Option<config::WindowBounds>>,
    /// 已落盘的位置大小（去重，避免无变化时反复写盘）
    pub(crate) saved_bounds: Mutex<Option<config::WindowBounds>>,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self {
            clickthrough: Mutex::new(false),
            always_on_top: Mutex::new(true),
            style: Mutex::new(config::OverlayStyle::default()),
            filter: Mutex::new(config::DanmakuFilter::default()),
            bounds: Mutex::new(None),
            saved_bounds: Mutex::new(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn ago(now: Instant, secs: u64) -> Instant {
        now - Duration::from_secs(secs)
    }

    #[test]
    fn 弹幕速率窗口只保留窗口内的时刻() {
        let now = Instant::now();
        let mut ticks = VecDeque::from(vec![
            ago(now, 15),
            ago(now, RATE_WINDOW_SECS), // 恰好 10s：已在窗口外
            ago(now, 9),
            now,
        ]);
        prune_ticks(&mut ticks, now);
        assert_eq!(ticks.len(), 2);
    }

    #[test]
    fn 弹幕速率裁剪空队列不报错() {
        let mut ticks = VecDeque::new();
        prune_ticks(&mut ticks, Instant::now());
        assert!(ticks.is_empty());
    }

    #[test]
    fn 弹幕速率全部过期时清空() {
        let now = Instant::now();
        let mut ticks = VecDeque::from(vec![ago(now, 30), ago(now, 11)]);
        prune_ticks(&mut ticks, now);
        assert!(ticks.is_empty());
    }
}
