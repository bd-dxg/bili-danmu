//! 弹幕朗读：Edge TTS 引擎 + 流水线播放队列
//!
//! 与弹幕显示完全解耦（prd.md §25）：显示过滤在前端 Overlay 做，朗读过滤在 `text.rs` 做，
//! 二者用各自独立的 `DanmakuFilter` 实例，互不影响。
//!
//! 数据流：弹幕事件 → 开关/筛选判定 → 文案清洗 → 有界队列 → 合成 →（通道）→ 串行播放。
//! 合成与播放两个任务见 `worker.rs`；音色与协议见 `edge/`；文案清洗见 `text.rs`。

pub mod edge;
mod gift;
mod player;
mod text;
mod worker;

pub use worker::spawn_worker;

use crate::bilibili::event::{Backing, Danmaku};
use crate::config::{GiftTtsConfig, TtsConfig};
use crate::state::OverlayState;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;
use text::{build_backing_text, build_text, clean_for_speech, matches_filter};

/// 打断冷却：距上次打断不足这么久就不再作废在途音频
/// （否则高频房间合成刚起步就被反复作废，白忙一场）
const INTERRUPT_COOLDOWN: Duration = Duration::from_millis(1800);

/// 队列项：`force` 标记不受「弹幕朗读」总开关限制
/// （试听与礼物朗读都有自己的开关，不该被弹幕那一侧的总开关静音）
struct QueueItem {
    text: String,
    force: bool,
}

/// 朗读队列与配置（Tauri 管理状态）
pub struct TtsState {
    config: Mutex<TtsConfig>,
    /// 礼物朗读配置（独立于弹幕朗读开关）
    gift_config: Mutex<GiftTtsConfig>,
    /// 待合成队列（满时丢最旧）
    queue: Mutex<VecDeque<QueueItem>>,
    /// 礼物连击聚合（静默满窗口才朗读，见 `gift.rs`）
    gift: Mutex<gift::GiftAggregator>,
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
            gift_config: Mutex::new(GiftTtsConfig::default()),
            queue: Mutex::new(VecDeque::new()),
            gift: Mutex::new(gift::GiftAggregator::default()),
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

    pub fn gift_config(&self) -> GiftTtsConfig {
        self.gift_config.lock().unwrap().clone()
    }

    /// 更新礼物朗读配置：关掉时丢弃还没念出去的连击分组
    pub fn set_gift_config(&self, config: GiftTtsConfig) {
        if !config.enabled {
            self.gift.lock().unwrap().clear();
        }
        *self.gift_config.lock().unwrap() = config;
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
            // 只丢待朗读的弹幕：试听与礼物朗读有自己的开关，不被弹幕侧的热开关连带清掉
            self.queue.lock().unwrap().retain(|item| item.force);
            // 递增代次作废在途音频（含正在合成的那条）；在播的那条由 play_mp3 的 stop_now 验开关停下
            self.epoch.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// 入队；队列满时丢弃最旧的待朗读项（高速直播间不堆积）
    fn push(&self, text: String, max_queue: u32, force: bool) {
        let mut queue = self.queue.lock().unwrap();
        while queue.len() >= max_queue.max(1) as usize {
            // 丢最旧的**弹幕**：队首可能就是插队的礼物，直接 pop_front 会把它顶掉
            // （高频直播间每秒几十条弹幕，上舰/醒目留言这种只来一次的大额插队后往往活不到开播）
            match queue.iter().position(|item| !item.force) {
                Some(i) => {
                    queue.remove(i);
                }
                // 队列里全是插队项：只能丢最旧的那个
                None => {
                    queue.pop_front();
                }
            }
        }
        queue.push_back(QueueItem { text, force });
        drop(queue);
        self.notify.notify_one();
    }

    /// 插队入队（礼物朗读）：放到队首，排在所有待朗读弹幕之前。
    /// 队列满时丢最旧的：待朗读的弹幕比打赏更可等，打赏不该被挤出队列。
    fn push_front(&self, text: String, max_queue: u32) {
        let mut queue = self.queue.lock().unwrap();
        while queue.len() >= max_queue.max(1) as usize {
            queue.pop_front();
        }
        queue.push_front(QueueItem { text, force: true });
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

/// 打赏到达钩子（非阻塞，在弹幕事件回调里调用）
///
/// 连击类先攒着（静默满一个连击窗口才念，否则连送 20 个就念 20 遍），
/// 醒目留言与上舰不连击，立刻插队朗读。
pub fn on_backing(app: &AppHandle, b: &Backing) {
    let state = app.state::<TtsState>();
    // 关闭礼物朗读时也照常聚合：开关与门槛在 `speak_backing` 里判，
    // 到期没念出去的分组会自行丢弃（不会积压）
    let immediate = state
        .gift
        .lock()
        .unwrap()
        .on_backing(b.clone(), Instant::now());
    if let Some(row) = immediate {
        speak_backing(app, &row);
    }
}

/// 把一条打赏插队送进朗读队列（开关开启且过了朗读门槛才念）
fn speak_backing(app: &AppHandle, b: &Backing) {
    let state = app.state::<TtsState>();
    let cfg = state.gift_config();
    if !cfg.enabled || b.amount_fen < gift::threshold_fen(&cfg) {
        return;
    }
    let text = build_backing_text(b);
    if text.is_empty() {
        return;
    }
    state.push_front(text, state.config().max_queue);
}

/// 礼物朗读定时器：常驻任务，每「连击窗口」醒一次，把静默满窗口的分组念出去
///
/// 不给每个分组各起一个延时任务：连击 100 次会留下 99 个空转任务，还有
/// 「定时器退出」与「事件到达」的竞态；常驻轮询每轮只加一次锁，代价可忽略。
pub(super) fn spawn_gift_timer(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            // 1 秒一跳（不是按窗口整跳）：否则汇总最多晚一个窗口才能念——
            // 礼物在 t=1 到达、t=5 醒来时静默才 4 秒，得再等一整轮
            tokio::time::sleep(Duration::from_secs(1)).await;
            let window = combo_window(&app);
            let due = {
                let state = app.state::<TtsState>();
                let cfg = state.gift_config();
                let mut agg = state.gift.lock().unwrap();
                agg.take_due(Instant::now(), window, &cfg)
            };
            for row in due {
                speak_backing(&app, &row);
            }
        }
    });
}

/// 连击合并窗口：沿用礼物渲染侧的配置（同一个直播间里「连击」应是同一口径）
fn combo_window(app: &AppHandle) -> Duration {
    let secs = app
        .state::<OverlayState>()
        .gift
        .lock()
        .unwrap()
        .combo_window_secs;
    Duration::from_secs(secs.max(1))
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

    #[test]
    fn 插队的礼物排在待朗读弹幕之前() {
        let state = TtsState::new(TtsConfig::default());
        state.push("弹幕一".into(), 5, false);
        state.push("弹幕二".into(), 5, false);
        state.push_front("感谢老板A送的5个辣条".into(), 5);
        let queue = state.queue.lock().unwrap();
        assert_eq!(queue.front().unwrap().text, "感谢老板A送的5个辣条");
        assert_eq!(queue.len(), 3);
    }

    #[test]
    fn 插队的礼物不会被队列上限挤掉() {
        let state = TtsState::new(TtsConfig::default());
        for i in 0..5 {
            state.push(format!("弹幕{i}"), 2, false);
        }
        // 队列已满（上限 2）：插队时丢的是最旧的弹幕，礼物自己一定在队首
        state.push_front("感谢老板A送的5个辣条".into(), 2);
        let queue = state.queue.lock().unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.front().unwrap().text, "感谢老板A送的5个辣条");
    }

    #[test]
    fn 高频弹幕不会把插队的礼物顶掉() {
        // 队列上限 2：插队的是上舰（只来一次），后面弹幕连续涌入
        let state = TtsState::new(TtsConfig::default());
        state.push_front("感谢木茂上舰舰长".into(), 2);
        for i in 0..5 {
            state.push(format!("弹幕{i}"), 2, false);
        }
        let queue = state.queue.lock().unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.front().unwrap().text, "感谢木茂上舰舰长");
        assert_eq!(queue.back().unwrap().text, "弹幕4", "丢的应是最旧的弹幕");
    }

    #[test]
    fn 关闭总开关时保留试听与礼物朗读() {
        let state = TtsState::new(TtsConfig::default());
        state.push("弹幕一".into(), 5, false);
        state.push_front("感谢老板A送的5个辣条".into(), 5);
        state.set_config(TtsConfig {
            enabled: false,
            ..TtsConfig::default()
        });
        let queue = state.queue.lock().unwrap();
        assert_eq!(queue.len(), 1, "只该丢掉待朗读的弹幕");
        assert_eq!(queue.front().unwrap().text, "感谢老板A送的5个辣条");
    }

    #[test]
    fn 关闭礼物朗读时丢弃未念的分组() {
        let state = TtsState::new(TtsConfig::default());
        let b = crate::bilibili::event::Backing {
            id: "1".into(),
            kind: crate::bilibili::event::BackingKind::Gift,
            uid: 1,
            username: "老板A".into(),
            gift_name: "辣条".into(),
            gift_id: 100,
            num: 1,
            amount_fen: 100,
            timestamp: 0,
            message: None,
            guard_level: None,
            wealth_level: None,
        };
        state
            .gift
            .lock()
            .unwrap()
            .on_backing(b, Instant::now());
        state.set_gift_config(GiftTtsConfig {
            enabled: false,
            min_amount_yuan: 0.0,
        });
        let due = state.gift.lock().unwrap().take_due(
            Instant::now() + Duration::from_secs(10),
            Duration::from_secs(1),
            &GiftTtsConfig::default(),
        );
        assert!(due.is_empty());
    }
}
