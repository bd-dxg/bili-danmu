//! 欢迎消息：进房 / 关注 / 分享 / 点赞 / 舰长进场的限流与去重
//!
//! 这几类事件的量级比弹幕大（实测进房 : 弹幕在 1.5 : 1 ~ 10 : 1 之间，随房间差很多：
//! 狼人杀 58 万在线是 160 : 103，另一间 1.1 万在线的房间是 103 : 10——后者弹幕特别稀，
//! 比值被拉大），原样广播会把弹幕窗的 120 条上限吃光、真弹幕全被顶掉，所以先过两道闸：
//!
//! 1. 去重：同一个人在 `DEDUPE_SECS` 内只出一条（离开再进、连点、多标签页都会刷）
//! 2. 限速：每 `RATE_WINDOW` 最多 `MAX_PER_WINDOW` 条
//!
//! 额度做成「一个大池 + 舰长进场一个小池」，而不是每种形态一份：分形态的话 4 类各一份，
//! 合起来就是二十几条/分钟，比中位房间的弹幕（实测 10 条/分钟）还密，那就是刷屏。
//! 舰长进场单独一份而不是完全不限：它稀缺且重要，但热度房里实测也有 10 条/分钟
//! （58 万在线房间 120 秒 21 条 ENTRY_EFFECT），真豁免就会反超弹幕成为占比大头。
//!
//! 丢掉的欢迎消息不补发——它是「热闹感」，不是必须送达的消息。

use crate::bilibili::event::{Welcome, WelcomeKind};
use crate::state::OverlayState;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// 同一人同类事件的最小间隔（秒）
const DEDUPE_SECS: u64 = 60;
/// 限速窗口
const RATE_WINDOW: Duration = Duration::from_secs(30);
/// 每个限速窗口内最多放行条数（≈ 2 条/分钟）
const MAX_PER_WINDOW: usize = 1;
/// 去重表容量上限
const DEDUPE_CAP: usize = 4096;

/// 欢迎消息运行态（Tauri 托管）
#[derive(Default)]
pub(crate) struct WelcomeState {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// (形态, uid) → 上次放行时间
    seen: HashMap<(WelcomeKind, i64), Instant>,
    /// 普通形态共用的额度（进房 / 关注 / 分享 / 点赞）
    slots: VecDeque<Instant>,
    /// 舰长进场自己的额度
    guard_slots: VecDeque<Instant>,
}

impl Inner {
    /// 判定这条欢迎消息是否放行
    fn admit(&mut self, w: &Welcome, now: Instant) -> bool {
        // 1. 去重：同一个人同类事件在窗口内只出一条
        let key = (w.kind, w.uid);
        if let Some(last) = self.seen.get(&key) {
            if now.duration_since(*last) < Duration::from_secs(DEDUPE_SECS) {
                return false;
            }
        }
        // 2. 限速：舰长进场走自己的小池，其余共用一个池
        let quota = if w.kind == WelcomeKind::GuardEnter {
            &mut self.guard_slots
        } else {
            &mut self.slots
        };
        while quota
            .front()
            .is_some_and(|t| now.duration_since(*t) >= RATE_WINDOW)
        {
            quota.pop_front();
        }
        if quota.len() >= MAX_PER_WINDOW {
            return false;
        }
        quota.push_back(now);
        // 去重记录只值几十秒，满了整体清空即可，不值得为它做 LRU
        if self.seen.len() >= DEDUPE_CAP {
            self.seen.clear();
        }
        self.seen.insert(key, now);
        true
    }
}

/// 处理一条欢迎消息 → 去重限速 → 广播 `welcome` 事件给弹幕窗
///
/// 开关为关时直接返回，连去重表都不动。
pub(crate) fn on_welcome(app: &AppHandle, w: Welcome) {
    if !app.state::<OverlayState>().welcome.lock().unwrap().enabled {
        return;
    }
    // 锁内只判定不广播：emit 是跨 webview 的分发，压在状态锁里迟早把锁序搅乱
    let pass = {
        let st = app.state::<WelcomeState>();
        let mut inner = st.inner.lock().unwrap();
        inner.admit(&w, Instant::now())
    };
    if pass {
        let _ = app.emit("welcome", &w);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn welcome(kind: WelcomeKind, uid: i64) -> Welcome {
        Welcome {
            id: format!("t-{uid}"),
            kind,
            uid,
            username: "观众A".into(),
            timestamp: 1700000300,
            guard_level: None,
        }
    }

    #[test]
    fn 同一人去重窗口内只过一条() {
        let mut inner = Inner::default();
        let now = Instant::now();
        assert!(inner.admit(&welcome(WelcomeKind::Enter, 1), now));
        // 31 秒后：已出限速窗口，但仍在 60 秒去重窗口内 → 挡在去重这一关
        assert!(!inner.admit(
            &welcome(WelcomeKind::Enter, 1),
            now + Duration::from_secs(31)
        ));
        // 61 秒后：两个窗口都过期
        assert!(inner.admit(
            &welcome(WelcomeKind::Enter, 1),
            now + Duration::from_secs(61)
        ));
    }

    #[test]
    fn 全局额度用完后丢弃() {
        let mut inner = Inner::default();
        let now = Instant::now();
        assert!(inner.admit(&welcome(WelcomeKind::Enter, 1), now));
        assert!(
            !inner.admit(&welcome(WelcomeKind::Enter, 2), now + Duration::from_secs(1)),
            "额度内第二个人也要丢"
        );
        assert!(
            inner.admit(&welcome(WelcomeKind::Enter, 3), now + Duration::from_secs(31)),
            "出窗口后放行"
        );
    }

    #[test]
    fn 不同形态共用同一份全局额度() {
        let mut inner = Inner::default();
        let now = Instant::now();
        assert!(inner.admit(&welcome(WelcomeKind::Enter, 1), now));
        assert!(
            !inner.admit(&welcome(WelcomeKind::Follow, 2), now),
            "换形态不额外给额度——这正是分形态限速会刷屏的原因"
        );
        assert!(!inner.admit(&welcome(WelcomeKind::Like, 3), now));
    }

    #[test]
    fn 舰长进场有自己的额度不占用普通额度() {
        let mut inner = Inner::default();
        let now = Instant::now();
        assert!(inner.admit(&welcome(WelcomeKind::GuardEnter, 1), now));
        assert!(
            !inner.admit(&welcome(WelcomeKind::GuardEnter, 2), now),
            "热度房里 ENTRY_EFFECT 也有 10 条/分钟，不能真豁免"
        );
        assert!(
            inner.admit(&welcome(WelcomeKind::Enter, 3), now),
            "舰长进场占的是自己的池，不该挤掉进房的额度"
        );
        assert!(
            inner.admit(&welcome(WelcomeKind::GuardEnter, 4), now + Duration::from_secs(31)),
            "出窗口后舰长进场恢复"
        );
    }

    #[test]
    fn 去重表满后整体清空() {
        let mut inner = Inner::default();
        let now = Instant::now();
        for uid in 0..=(DEDUPE_CAP as i64) {
            // 舰长进场要走自己的池，不然 31 秒额度会被限速挡住
            inner.admit(&welcome(WelcomeKind::GuardEnter, uid), now + Duration::from_secs(uid as u64 * 31));
        }
        assert!(
            inner.seen.len() <= DEDUPE_CAP,
            "清空后表长应回落到容量以内"
        );
    }
}
