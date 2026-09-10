//! 弹幕朗读：Edge TTS 引擎 + 流水线播放队列
//!
//! 与弹幕显示完全解耦（prd.md §25）：显示过滤在前端 Overlay 做，朗读过滤在这里做，
//! 二者用各自独立的 `DanmakuFilter` 实例，互不影响。
//!
//! 数据流：弹幕事件 → 开关/筛选判定 → 文案清洗 → 有界队列 → 合成 →（通道）→ 串行播放。
//! 合成与播放是两个任务：播第 N 条时已经在合成第 N+1 条，省掉每条的合成等待；
//! 播放严格串行，不会有多段语音重叠（prd.md §22）。

pub mod edge;
mod player;

use crate::bilibili::event::Danmaku;
use crate::config::{DanmakuFilter, TtsConfig};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc, Notify};

/// 同一字符连续重复的最大保留次数（「哈哈哈哈哈哈哈」只读三个）
const MAX_REPEAT: u32 = 3;
/// 合成 → 播放通道容量：1 = 最多预合成 1 条
const PIPELINE_DEPTH: usize = 1;
/// 打断冷却：距上次打断不足这么久就不再打断，保证每条至少能念出头一句
/// （否则高速房间会变成一直被打断、永远听不到完整句子）
const INTERRUPT_COOLDOWN: Duration = Duration::from_millis(1800);

/// 队列项：`force` 标记的试听不受总开关限制
struct QueueItem {
    text: String,
    force: bool,
}

/// 已合成的音频；带 `epoch`，被新弹幕顶掉（打断）后作废
struct AudioChunk {
    data: Vec<u8>,
    epoch: u64,
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

    /// 当前音色列表（内置兜底 + 已拉取的完整列表）
    pub fn voices(&self) -> Vec<(String, String)> {
        self.voices.lock().unwrap().clone()
    }

    pub fn set_voices(&self, voices: Vec<(String, String)>) {
        *self.voices.lock().unwrap() = voices;
    }

    /// 更新配置：关闭时立即丢弃积压并掐断当前朗读
    pub fn set_config(&self, config: TtsConfig) {
        let disabled = !config.enabled;
        *self.config.lock().unwrap() = config;
        if disabled {
            self.queue.lock().unwrap().clear();
            self.epoch.fetch_add(1, Ordering::Relaxed);
            player::stop();
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

    /// 积压时打断：已在出声且队列里还有更新的弹幕，就掐掉当前这条
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
        // 递增代次：正在合成与已合成未播的那条都会因代次不符被丢弃
        self.epoch.fetch_add(1, Ordering::Relaxed);
        player::stop();
    }
}

/// 启动朗读任务（应用启动时调用一次）：合成与播放各一个任务，之间用容量 1 的通道衔接
pub fn spawn_worker(app: AppHandle) {
    let (play_tx, mut play_rx) = mpsc::channel::<AudioChunk>(PIPELINE_DEPTH);

    // 播放任务：严格串行；代次过期的音频（已被更新弹幕顶掉）直接丢弃
    let player_app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(chunk) = play_rx.recv().await {
            let current = player_app.state::<TtsState>().epoch.load(Ordering::Relaxed);
            if chunk.epoch != current {
                continue;
            }
            player_app
                .state::<TtsState>()
                .playing
                .store(true, Ordering::Relaxed);
            // MCI 播放要阻塞到播完，放阻塞线程池里跑，别占住异步 worker
            let _ = tokio::task::spawn_blocking(move || player::play_mp3(&chunk.data)).await;
            player_app
                .state::<TtsState>()
                .playing
                .store(false, Ordering::Relaxed);
        }
    });

    // 合成任务：send 完立刻开始合成下一条，与播放重叠
    tauri::async_runtime::spawn(async move {
        loop {
            let item = app.state::<TtsState>().pop().await;
            let config = app.state::<TtsState>().config();
            // 关闭期间积压的弹幕在重新开启后不再补读（试听例外）
            if !config.enabled && !item.force {
                continue;
            }
            let epoch = app.state::<TtsState>().epoch.load(Ordering::Relaxed);
            match edge::synthesize(&config.voice, config.rate_pct, config.volume_pct, &item.text)
                .await
            {
                Ok(mp3) => {
                    let state = app.state::<TtsState>();
                    // 合成期间可能已被关掉或被打断，丢弃过期产物，别出声音
                    let keep = (item.force || state.config().enabled)
                        && state.epoch.load(Ordering::Relaxed) == epoch;
                    drop(state);
                    if keep && play_tx.send(AudioChunk { data: mp3, epoch }).await.is_err() {
                        break; // 播放端已退出
                    }
                }
                // 单条失败只记录不重试：限流/断网时重试会让日志和请求雪崩
                Err(e) => eprintln!("[tts] 合成失败：{e}"),
            }
        }
    });
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

/// 朗读筛选判定：与前端 Overlay 的显示筛选同语义
/// （身份规则之间是「或」，全部关闭 = 不过滤；敏感词屏蔽独立叠加且不豁免）
fn matches_filter(d: &Danmaku, f: &DanmakuFilter) -> bool {
    if f.enable_sensitive {
        let content = d.content.to_lowercase();
        if f.sensitive_words
            .iter()
            .any(|w| !w.is_empty() && content.contains(&w.to_lowercase()))
        {
            return false;
        }
    }
    if !(f.enable_guard_admin || f.enable_medal || f.enable_wealth) {
        return true;
    }
    (f.enable_guard_admin && (d.is_admin || d.guard_level.unwrap_or(0) >= 1))
        || (f.enable_medal && d.medal_level.unwrap_or(0) > 0)
        || (f.enable_wealth && d.wealth_level.unwrap_or(0) >= f.wealth_min)
}

/// 组装朗读文案：可选身份前缀 + 可选用户名 + 清洗后的正文
///
/// 正文没有可读内容（纯 emoji / 纯标点 / 纯链接）时整条返回空，
/// 不能只剩下「舰长 小明说」这种没内容的空壳。
/// `max_len` 只约束正文：前缀与用户名是固定少量字，不该挤掉弹幕本身。
fn build_text(d: &Danmaku, config: &TtsConfig) -> String {
    let mut content = clean_for_speech(&d.content);
    if !has_readable_content(&content) {
        return String::new();
    }
    let max = config.max_len as usize;
    if max > 0 && content.chars().count() > max {
        content = content.chars().take(max).collect();
    }

    let mut out = String::new();
    if config.read_role {
        if d.is_admin {
            out.push_str("房管");
        } else if let Some(guard) = d.guard_level.filter(|g| *g >= 1) {
            out.push_str(match guard {
                3 => "舰长",
                2 => "提督",
                _ => "总督",
            });
        }
        if !out.is_empty() {
            out.push(' ');
        }
    }
    if config.read_username {
        // 用户名同正文一样要剔噪声字符：昵称里的 `_` 同样会被念成「下划线」
        let name = strip_noise(&d.username);
        let name = name.trim();
        if has_readable_content(name) {
            out.push_str(name);
            out.push_str("说 ");
        }
    }
    out.push_str(&content);
    out.trim().to_string()
}

/// 是否有可读内容：纯标点（`?` `？` `！` `~`）、纯 emoji、纯空白都算「没东西可念」。
/// 汉字与数字在 Unicode 里属于字母/数字，「哈哈哈」「666」都算有内容。
fn has_readable_content(text: &str) -> bool {
    text.chars().any(|ch| ch.is_alphanumeric())
}

/// 朗读前清洗（prd.md §24）：丢掉噪声字符与链接、折叠连续重复字符、规整空白
fn clean_for_speech(text: &str) -> String {
    collapse_repeats(&strip_noise(text))
        .split_whitespace()
        .filter(|token| !is_link(token))
        .collect::<Vec<_>>()
        .join(" ")
}

/// 只剔除噪声字符（昵称这类短文本用：不该折叠重复字符，也不该丢链接）
fn strip_noise(text: &str) -> String {
    text.chars().filter(|ch| !is_noise(*ch)).collect()
}

/// 同一字符连续超过 MAX_REPEAT 次的部分丢掉（「哈哈哈哈哈哈哈」只读三个）
fn collapse_repeats(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev = '\0';
    let mut repeat = 0u32;
    for ch in text.chars() {
        if ch == prev {
            repeat += 1;
            if repeat > MAX_REPEAT {
                continue;
            }
        } else {
            prev = ch;
            repeat = 1;
        }
        out.push(ch);
    }
    out
}

/// 会被引擎念出「名字」而不是当作内容的字符。
///
/// 本机对 zh-CN-XiaoxiaoNeural 逐字实测得出（`scripts/edge-tts-probe.ps1`，Δ 字节数见 `Task/findings.md`）：
/// - `_`/`＿`→下划线、`\`/`＼`→反斜杠、`#`/`＃`→井号、`*`/`＊`→星号、`=`/`＝`→等号
/// - `%`/`％`→百分号、`$`/`＄`→美元、`@`/`＠`→at、`°`→度
/// - `ω`→欧米伽、`ε`→epsilon、`Σ`→sigma、`∀`→任意、`ψ`/`π`→字母名（颜文字常客）
///
/// 实测全角与半角行为完全一致，故两套一并列出。实测发音**有意义**、故保留的：
/// `×`→乘以、`÷`→除以、`±`→正负、`○`→零；实测**不出声**、无需处理的：
/// `~ ～ ! ? - . , / | ^ < > [ ] ( ) { } : ; · ・ … ※ ★ ☆ □ △ ▲ ● ◇ ◆ ▁ ﹏ ╯ ╰ ﾟ ｀`
const NAMED_CHARS: &str = "_\\#*=%@$°＿＼＃＊＝％＄＠ωεΣ∀ψπ";

/// 朗读前丢弃的字符：控制字符、emoji / 图形符号、以及会被念出名字的字符
fn is_noise(ch: char) -> bool {
    if NAMED_CHARS.contains(ch) {
        return true;
    }
    let code = ch as u32;
    code < 0x20
        || code == 0x7F
        || code == 0x200D
        || code == 0xFFFD
        || (0xFE00..=0xFE0F).contains(&code)
        || (0x2190..=0x21FF).contains(&code)
        || (0x2600..=0x27BF).contains(&code)
        || (0x2B00..=0x2BFF).contains(&code)
        || (0x1F000..=0x1FAFF).contains(&code)
}

/// 网址不值得念，整段丢掉
fn is_link(token: &str) -> bool {
    let lower = token.to_lowercase();
    lower.starts_with("http")
        || lower.starts_with("www.")
        || lower.contains("://")
        || lower.contains(".com/")
        || lower.contains(".cn/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn danmaku(content: &str) -> Danmaku {
        Danmaku {
            id: "1".into(),
            username: "小明".into(),
            content: content.into(),
            timestamp: 0,
            color: None,
            medal_level: None,
            medal_name: None,
            medal_room_id: None,
            user_level: None,
            guard_level: None,
            wealth_level: None,
            is_admin: false,
        }
    }

    #[test]
    fn collapses_repeated_characters() {
        assert_eq!(clean_for_speech("哈哈哈哈哈哈哈哈"), "哈哈哈");
        assert_eq!(clean_for_speech("6"), "6");
        assert_eq!(clean_for_speech("6666666到飞起"), "666到飞起");
    }

    #[test]
    fn strips_emoji_and_links_and_whitespace() {
        assert_eq!(clean_for_speech("主播好棒😄🎉"), "主播好棒");
        assert_eq!(clean_for_speech("看这里 https://b23.tv/abc 谢谢"), "看这里 谢谢");
        assert_eq!(clean_for_speech("  多个   空格  "), "多个 空格");
    }

    #[test]
    fn strips_symbols_the_engine_would_name_out_loud() {
        // `_` 会被念成「下划线」：只忽略这个字符，整条弹幕照读
        assert_eq!(clean_for_speech("主播_好棒"), "主播好棒");
        assert_eq!(clean_for_speech("_顶_你_"), "顶你");
        assert_eq!(clean_for_speech("C_语言 ＃1 星＊号"), "C语言 1 星号");
        // 只由这类符号组成 → 确实没东西可念，交由上层丢弃
        assert!(clean_for_speech("_＿_＼").is_empty());
        // 实测不出声的符号保留（波浪号是语气延伸，不该删）
        assert_eq!(clean_for_speech("你好~!!"), "你好~!!");
        // 颜文字里的希腊字母会被念成字母名（「欧米伽」），度号会被念成「度」
        assert_eq!(clean_for_speech("(╯°□°)╯ε"), "(╯□)╯");
    }

    #[test]
    fn symbols_do_not_break_repeat_collapsing() {
        assert_eq!(clean_for_speech("哈哈哈_哈哈哈"), "哈哈哈");
    }

    #[test]
    fn build_text_follows_output_switches() {
        let plain = TtsConfig::default();
        let d = danmaku("谢谢礼物");
        assert_eq!(build_text(&d, &plain), "谢谢礼物");

        let with_user = TtsConfig {
            read_username: true,
            ..TtsConfig::default()
        };
        assert_eq!(build_text(&d, &with_user), "小明说 谢谢礼物");
    }

    #[test]
    fn build_text_prefixes_role_and_truncates() {
        let config = TtsConfig {
            read_role: true,
            max_len: 3,
            ..TtsConfig::default()
        };
        let mut d = danmaku("谢谢谢谢礼物");
        d.guard_level = Some(3);
        // 折叠后正文是「谢谢谢礼物」，截到 3 字；前缀「舰长」不计入上限
        assert_eq!(build_text(&d, &config), "舰长 谢谢谢");
    }

    #[test]
    fn max_len_counts_content_only() {
        // 长昵称 + 身份前缀不能挤掉正文额度
        let config = TtsConfig {
            read_role: true,
            read_username: true,
            max_len: 5,
            ..TtsConfig::default()
        };
        let mut d = danmaku("今天天气真不错呢");
        d.username = "森罗万象之主宰".into();
        d.guard_level = Some(3);
        assert_eq!(
            build_text(&d, &config),
            "舰长 森罗万象之主宰说 今天天气真"
        );
    }

    #[test]
    fn cleans_underscores_in_username_too() {
        // 昵称 `雨月云__` 里的下划线同样会被念成「下划线」，昵称也走噪声剔除
        let config = TtsConfig {
            read_username: true,
            ..TtsConfig::default()
        };
        let mut d = danmaku("好好好");
        d.username = "雨月云__".into();
        assert_eq!(build_text(&d, &config), "雨月云说 好好好");
    }

    #[test]
    fn skips_username_that_is_all_noise() {
        let config = TtsConfig {
            read_username: true,
            ..TtsConfig::default()
        };
        let mut d = danmaku("好好好");
        d.username = "_＿_".into();
        assert_eq!(build_text(&d, &config), "好好好");
    }

    #[test]
    fn user_name_keeps_repeats() {
        // 昵称里的重复字符不折叠（那是用户的名字，不是拖长音）
        assert_eq!(strip_noise("哈哈哈哈哈__"), "哈哈哈哈哈");
    }

    #[test]
    fn empty_content_after_cleaning_is_not_spoken_at_all() {
        // 正文没东西可念时，不该只剩下「舰长 小明说」这种空壳
        let config = TtsConfig {
            read_role: true,
            read_username: true,
            ..TtsConfig::default()
        };
        let mut d = danmaku("😄🎉");
        d.guard_level = Some(3);
        assert_eq!(build_text(&d, &config), "");
        assert_eq!(build_text(&danmaku("__"), &config), "");
        assert_eq!(build_text(&danmaku("https://b23.tv/abc"), &config), "");
        assert_eq!(build_text(&danmaku("   "), &config), "");
    }

    #[test]
    fn punctuation_only_content_is_not_spoken_at_all() {
        // 「？」「！」这类符号引擎不出声，只剩前缀就会念成「小明说」+ 空白
        let config = TtsConfig {
            read_role: true,
            read_username: true,
            ..TtsConfig::default()
        };
        let mut d = danmaku("?");
        d.guard_level = Some(3);
        assert_eq!(build_text(&d, &config), "");
        assert_eq!(build_text(&danmaku("？？？"), &config), "");
        assert_eq!(build_text(&danmaku("！ 。 ~"), &config), "");
        // 标点里夹了字就得念，只把无内容的标点留在后面无妨
        assert_eq!(build_text(&danmaku("?好"), &config), "小明说 ?好");
    }

    #[test]
    fn punctuation_only_username_is_skipped() {
        let config = TtsConfig {
            read_username: true,
            ..TtsConfig::default()
        };
        let mut d = danmaku("谢谢你");
        d.username = "？？".into();
        assert_eq!(build_text(&d, &config), "谢谢你");
    }

    #[test]
    fn sensitive_word_drops_whole_danmaku_including_prefix() {
        let config = TtsConfig {
            read_role: true,
            read_username: true,
            filter: DanmakuFilter {
                enable_sensitive: true,
                sensitive_words: vec!["哈哈".into()],
                ..DanmakuFilter::default()
            },
            ..TtsConfig::default()
        };
        let mut d = danmaku("哈哈哈哈");
        d.guard_level = Some(3);
        // 筛选在组装文案之前：命中敏感词就整条丢掉，不会念出「舰长 小明说」
        assert!(!matches_filter(&d, &config.filter));
    }

    #[test]
    fn filter_matches_chosen_identities() {
        let admins_only = DanmakuFilter {
            enable_guard_admin: true,
            ..DanmakuFilter::default()
        };
        let mut d = danmaku("你好");
        assert!(!matches_filter(&d, &admins_only));
        d.is_admin = true;
        assert!(matches_filter(&d, &admins_only));
        // 敏感词屏蔽独立叠加，命中时房管也不豁免
        let blocked = DanmakuFilter {
            enable_guard_admin: true,
            enable_sensitive: true,
            sensitive_words: vec!["广告".into()],
            ..DanmakuFilter::default()
        };
        assert!(!matches_filter(&danmaku("加群 广告"), &blocked));
    }

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
