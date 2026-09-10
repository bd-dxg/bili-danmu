//! 弹幕朗读：Edge TTS 引擎 + 流水线播放队列
//!
//! 与弹幕显示完全解耦（prd.md §25）：显示过滤在前端 Overlay 做，朗读过滤在 `text.rs` 做，
//! 二者用各自独立的 `DanmakuFilter` 实例，互不影响。
//!
//! 数据流：弹幕事件 → 开关/筛选判定 → 文案清洗 → 有界队列 → 合成 →（通道）→ 串行播放。
//! 合成与播放两个任务见 `worker.rs`；音色与协议见 `edge/`；文案清洗见 `text.rs`。

pub mod edge;
mod player;
mod text;
mod worker;

pub use worker::spawn_worker;

use crate::bilibili::event::Danmaku;
use crate::config::TtsConfig;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;
use text::{build_text, clean_for_speech, matches_filter};

/// 打断冷却：距上次打断不足这么久就不再作废在途音频
/// （否则高频房间合成刚起步就被反复作废，白忙一场）
const INTERRUPT_COOLDOWN: Duration = Duration::from_millis(1800);

/// 队列项：`force` 标记的试听不受总开关限制
struct QueueItem {
    text: String,
    force: bool,
}

/// 朗读队列与配置（Tauri 管理状态）
pub struct TtsState {
    config: Mutex<TtsConfig>,
    /// 待合成队列（满时丢最旧）
    queue: Mutex<VecDeque<QueueItem>>,
    /// 唤醒合成端
    notify: Notify,
    /// 打断代次：递增即作废所有在途音频（含已合成未播与正在合成的那条）
    epoch: AtomicU64,
    /// 是否正在出声（决定打断是否有意义）
    playing: AtomicBool,
    /// 上次打断时刻（冷却用）
    last_interrupt: Mutex<Option<Instant>>,
    /// 可选音色 (ID, 显示名)：初值为内置中文音色，拉取成功后换成微软完整列表
    voices: Mutex<Vec<(String, String)>>,
}

impl TtsState {
    pub fn new(config: TtsConfig) -> Self {
        Self {
            config: Mutex::new(config),
            queue: Mutex::new(VecDeque::new()),
            notify: Notify::new(),
            epoch: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            last_interrupt: Mutex::new(None),
            voices: Mutex::new(edge::builtin_voices()),
        }
    }

    pub fn config(&self) -> TtsConfig {
        self.config.lock().unwrap().clone()
    }

    /// 总开关状态（播放端每个轮询查一次，决定要不要立刻停）
    pub fn is_enabled(&self) -> bool {
        self.config.lock().unwrap().enabled
    }

    /// 当前音色列表（内置兜底 + 已拉取的完整列表）
    pub fn voices(&self) -> Vec<(String, String)> {
        self.voices.lock().unwrap().clone()
    }

    pub fn set_voices(&self, voices: Vec<(String, String)>) {
        *self.voices.lock().unwrap() = voices;
    }

    /// 更新配置：关闭时丢弃积压（在播的那条由播放端查开关后自行停下）
    pub fn set_config(&self, config: TtsConfig) {
        let disabled = !config.enabled;
        *self.config.lock().unwrap() = config;
        if disabled {
            self.queue.lock().unwrap().clear();
            // 递增代次作废在途音频（含正在合成的那条）；在播的那条由 play_mp3 的 stop_now 验开关停下
            self.epoch.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 入队；队列满时丢弃最旧的待朗读项（高速直播间不堆积）
    fn push(&self, text: String, max_queue: u32, force: bool) {
        let mut queue = self.queue.lock().unwrap();
        while queue.len() >= max_queue.max(1) as usize {
            queue.pop_front();
        }
        queue.push_back(QueueItem { text, force });
        drop(queue);
        self.notify.notify_one();
    }

    /// 取一条待朗读项，队列空则挂起等待
    async fn pop(&self) -> QueueItem {
        loop {
            if let Some(item) = self.queue.lock().unwrap().pop_front() {
                return item;
            }
            // Notify 会保留一次许可，push 与 pop 竞态下不会丢唤醒
            self.notify.notified().await;
        }
    }

    /// 积压时打断：已在出声且队列里还有更新的弹幕，就作废在途音频
    /// （正在合成的 + 已合成未播的），让更新的弹幕尽快接上。
    /// **不**掐正在念的那条：音频整条合成，掐断只会听到半句（开了念用户名时连正文都没开始），
    /// 故延迟上限是一条朗读时长。
    fn interrupt_if_backlogged(&self, enabled: bool) {
        if !enabled || !self.playing.load(Ordering::Relaxed) {
            return;
        }
        if self.queue.lock().unwrap().is_empty() {
            return;
        }
        {
            let mut last = self.last_interrupt.lock().unwrap();
            if last.is_some_and(|t| t.elapsed() < INTERRUPT_COOLDOWN) {
                return;
            }
            *last = Some(Instant::now());
        }
        // 递增代次：正在合成、已合成未播的那条会因代次不符被丢弃（在播的那条念完）
        self.epoch.fetch_add(1, Ordering::Relaxed);
    }
}

/// 弹幕到达钩子：判定通过则清洗入队（非阻塞，在弹幕事件回调里调用）
pub fn on_danmaku(app: &AppHandle, d: &Danmaku) {
    let state = app.state::<TtsState>();
    let config = state.config.lock().unwrap();
    if !config.enabled || !matches_filter(d, &config.filter) {
        return;
    }
    let text = build_text(d, &config);
    let max_queue = config.max_queue;
    let interrupt = config.interrupt_on_backlog;
    drop(config);
    if text.is_empty() {
        return;
    }
    state.push(text, max_queue, false);
    state.interrupt_if_backlogged(interrupt);
}

/// 试听：不受总开关限制，直接把一段文本送进朗读队列（设置页调音色/语速用）
pub fn speak_test(app: &AppHandle, text: &str) {
    let text = clean_for_speech(text);
    if text.is_empty() {
        return;
    }
    app.state::<TtsState>().push(text, 1, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_drops_oldest_when_full() {
        let state = TtsState::new(TtsConfig::default());
        for i in 0..5 {
            state.push(format!("第{i}条"), 3, false);
        }
        let queue = state.queue.lock().unwrap();
        assert_eq!(queue.len(), 3);
        assert_eq!(queue.front().unwrap().text, "第2条");
    }

    #[test]
    fn interrupt_only_when_playing_with_backlog_and_outside_cooldown() {
        let state = TtsState::new(TtsConfig::default());
        state.push("新弹幕".into(), 5, false);

        // 没在出声 → 不打断
        state.interrupt_if_backlogged(true);
        assert_eq!(state.epoch.load(Ordering::Relaxed), 0);
        // 开关关闭 → 不打断
        state.playing.store(true, Ordering::Relaxed);
        state.interrupt_if_backlogged(false);
        assert_eq!(state.epoch.load(Ordering::Relaxed), 0);

        // 在出声 + 有积压 + 开关开启 → 打断一次
        state.interrupt_if_backlogged(true);
        assert_eq!(state.epoch.load(Ordering::Relaxed), 1);
        // 冷却期内不重复打断（否则会一直被打断，听不到完整句子）
        state.interrupt_if_backlogged(true);
        assert_eq!(state.epoch.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn no_interrupt_when_queue_empty() {
        let state = TtsState::new(TtsConfig::default());
        state.playing.store(true, Ordering::Relaxed);
        state.interrupt_if_backlogged(true);
        assert_eq!(state.epoch.load(Ordering::Relaxed), 0);
    }
}
