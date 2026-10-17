//! 欢迎消息：进房 / 关注 / 分享 / 点赞 / 舰长进场的限流与去重
//!
//! 这几类事件的量级比弹幕大（实测进房 : 弹幕在 1.5 : 1 ~ 10 : 1 之间，随房间差很多：
//! 狼人杀 58 万在线是 160 : 103，另一间 1.1 万在线的房间是 103 : 10——后者弹幕特别稀，
//! 比值被拉大），原样广播会把弹幕窗的 120 条上限吃光、真弹幕全被顶掉，所以先过两道闸：
//!
//! 1. 去重：同一个人在 `DEDUPE_SECS` 内只出一条（离开再进、连点、多标签页都会刷）
//! 2. 限速：每 `RATE_WINDOW` 最多 `MAX_PER_WINDOW` 条
//!
//! 去重按「形态族」算，不按 cmd 算：同一个舰长到场会同时下发 `INTERACT_WORD_V2`（进房）
//! 与 `ENTRY_EFFECT`（舰长进场，`max_delay_time` 最多几秒后补发），按 cmd 去重就会出两条
//! 「进入直播间」。两者合成一族后，后到的舰长进场**复用**进房那条的行 id（前端按 id 覆盖），
//! 于是只留一行，且档位不丢——若反过来让进房那条胜出，舰长信息就永远显示不出来。
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

/// 去重用的「形态族」：进房与舰长进场是同一个人到场的两条报文，合成一族
fn family(kind: WelcomeKind) -> WelcomeKind {
    match kind {
        WelcomeKind::GuardEnter => WelcomeKind::Enter,
        k => k,
    }
}

#[derive(Default)]
struct Inner {
    /// (形态族, uid) → 上次放行时间
    seen: HashMap<(WelcomeKind, i64), Instant>,
    /// 已放行的「到场」行：uid → (行 id, 是否已是舰长进场行)
    arrivals: HashMap<i64, (String, bool)>,
    /// 普通形态共用的额度（进房 / 关注 / 分享 / 点赞）
    slots: VecDeque<Instant>,
    /// 舰长进场自己的额度
    guard_slots: VecDeque<Instant>,
}

impl Inner {
    /// 判定这条欢迎消息是否放行；放行时可能改写 `w.id`（舰长进场升级进房那一行）
    fn admit(&mut self, w: &mut Welcome, now: Instant) -> bool {
        // 1. 去重：同一个人同一形态族在窗口内只出一条
        let key = (family(w.kind), w.uid);
        if let Some(last) = self.seen.get(&key) {
            if now.duration_since(*last) < Duration::from_secs(DEDUPE_SECS) {
                // 同一到场已经出过行：进房重复直接丢；舰长进场则把那一行升级成舰长行
                // （复用 id，前端按 id 覆盖，位置不变；已经是舰长行就不再重复覆盖）
                let Some((id, is_guard)) = self.arrivals.get(&w.uid) else {
                    return false;
                };
                if w.kind != WelcomeKind::GuardEnter || *is_guard {
                    return false;
                }
                w.id = id.clone();
                self.arrivals.insert(w.uid, (w.id.clone(), true));
                return true;
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
            self.arrivals.clear();
        }
        self.seen.insert(key, now);
        if family(w.kind) == WelcomeKind::Enter {
            let is_guard = w.kind == WelcomeKind::GuardEnter;
            self.arrivals.insert(w.uid, (w.id.clone(), is_guard));
        }
        true
    }
}

/// 处理一条欢迎消息 → 去重限速 → 分发给两个独立消费者
///
/// 去重与限速**与开关无关地先跑**：开关只决定「谁来消费」。若让开关去挡限速，
/// 关掉渲染时舰长进场就能绕过去重表，同一个舰长每次进出都被重复念。
pub(crate) fn on_welcome(app: &AppHandle, mut w: Welcome) {
    // 锁内只判定不广播：emit 是跨 webview 的分发，压在状态锁里迟早把锁序搅乱
    let pass = {
        let st = app.state::<WelcomeState>();
        let mut inner = st.inner.lock().unwrap();
        inner.admit(&mut w, Instant::now())
    };
    if !pass {
        return;
    }
    // 两个消费者各自判开关：朗读只认舰长进场（见 `tts::on_welcome`），渲染认总开关
    crate::tts::on_welcome(app, &w);
    // 锁先释放再广播：emit 是跨 webview 的分发，不该在持锁状态下做
    let render = app.state::<OverlayState>().welcome.lock().unwrap().enabled;
    if render {
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

    /// 判定并返回是否放行（不需要看被改写后的 id 时用）
    fn admit(inner: &mut Inner, w: Welcome, now: Instant) -> bool {
        let mut w = w;
        inner.admit(&mut w, now)
    }

    #[test]
    fn 同一人去重窗口内只过一条() {
        let mut inner = Inner::default();
        let now = Instant::now();
        assert!(admit(&mut inner, welcome(WelcomeKind::Enter, 1), now));
        // 31 秒后：已出限速窗口，但仍在 60 秒去重窗口内 → 挡在去重这一关
        assert!(!admit(
            &mut inner,
            welcome(WelcomeKind::Enter, 1),
            now + Duration::from_secs(31)
        ));
        // 61 秒后：两个窗口都过期
        assert!(admit(
            &mut inner,
            welcome(WelcomeKind::Enter, 1),
            now + Duration::from_secs(61)
        ));
    }

    #[test]
    fn 全局额度用完后丢弃() {
        let mut inner = Inner::default();
        let now = Instant::now();
        assert!(admit(&mut inner, welcome(WelcomeKind::Enter, 1), now));
        assert!(
            !admit(
                &mut inner,
                welcome(WelcomeKind::Enter, 2),
                now + Duration::from_secs(1)
            ),
            "额度内第二个人也要丢"
        );
        assert!(
            admit(
                &mut inner,
                welcome(WelcomeKind::Enter, 3),
                now + Duration::from_secs(31)
            ),
            "出窗口后放行"
        );
    }

    #[test]
    fn 不同形态共用同一份全局额度() {
        let mut inner = Inner::default();
        let now = Instant::now();
        assert!(admit(&mut inner, welcome(WelcomeKind::Enter, 1), now));
        assert!(
            !admit(&mut inner, welcome(WelcomeKind::Follow, 2), now),
            "换形态不额外给额度——这正是分形态限速会刷屏的原因"
        );
        assert!(!admit(&mut inner, welcome(WelcomeKind::Like, 3), now));
    }

    #[test]
    fn 舰长进场有自己的额度不占用普通额度() {
        let mut inner = Inner::default();
        let now = Instant::now();
        assert!(admit(&mut inner, welcome(WelcomeKind::GuardEnter, 1), now));
        assert!(
            !admit(&mut inner, welcome(WelcomeKind::GuardEnter, 2), now),
            "热度房里 ENTRY_EFFECT 也有 10 条/分钟，不能真豁免"
        );
        assert!(
            admit(&mut inner, welcome(WelcomeKind::Enter, 3), now),
            "舰长进场占的是自己的池，不该挤掉进房的额度"
        );
        assert!(
            admit(
                &mut inner,
                welcome(WelcomeKind::GuardEnter, 4),
                now + Duration::from_secs(31)
            ),
            "出窗口后舰长进场恢复"
        );
    }

    #[test]
    fn 高频灌入十分钟只放行二十条() {
        let mut inner = Inner::default();
        let now = Instant::now();
        // 模拟热度房：每 0.5 秒一条进房（实测热房间 80 条/分钟），持续 10 分钟，用户各不相同
        let mut admitted = 0usize;
        for i in 0..1200u64 {
            if admit(
                &mut inner,
                welcome(WelcomeKind::Enter, i as i64),
                now + Duration::from_millis(i * 500),
            ) {
                admitted += 1;
            }
        }
        assert_eq!(admitted, 20, "10 分钟 ÷ 30 秒窗口 = 20 条，与灌入速率无关");
    }

    #[test]
    fn 去重表满后整体清空() {
        let mut inner = Inner::default();
        let now = Instant::now();
        for uid in 0..=(DEDUPE_CAP as i64) {
            // 舰长进场要走自己的池，不然 31 秒额度会被限速挡住
            admit(
                &mut inner,
                welcome(WelcomeKind::GuardEnter, uid),
                now + Duration::from_secs(uid as u64 * 31),
            );
        }
        assert!(inner.seen.len() <= DEDUPE_CAP, "清空后表长应回落到容量以内");
    }

    #[test]
    fn 同一舰长的进房与舰长进场只出一行() {
        let mut inner = Inner::default();
        let now = Instant::now();
        // 1. 先进房（INTERACT_WORD_V2）
        assert!(admit(&mut inner, welcome(WelcomeKind::Enter, 7), now));
        // 2. 几秒后 ENTRY_EFFECT 补发：不能另起一行，要复用进房那行的 id 覆盖
        let mut g = welcome(WelcomeKind::GuardEnter, 7);
        g.guard_level = Some(3);
        assert!(
            inner.admit(&mut g, now + Duration::from_secs(2)),
            "舰长进场要把进房那行升级成舰长行"
        );
        assert_eq!(g.id, "t-7", "复用进房那行的 id，前端按 id 覆盖");
        // 3. 同一次到场再来一条舰长进场（B 站偶发重复推）：不再覆盖
        let mut again = welcome(WelcomeKind::GuardEnter, 7);
        again.guard_level = Some(3);
        assert!(!inner.admit(&mut again, now + Duration::from_secs(3)));
        // 4. 进房那条重推也丢
        assert!(!admit(
            &mut inner,
            welcome(WelcomeKind::Enter, 7),
            now + Duration::from_secs(4)
        ));
    }

    #[test]
    fn 舰长进场先到也能与随后的进房合成一行() {
        let mut inner = Inner::default();
        let now = Instant::now();
        let mut g = welcome(WelcomeKind::GuardEnter, 8);
        g.guard_level = Some(2);
        assert!(inner.admit(&mut g, now));
        // 进房那条晚到：同一到场，直接丢（已有舰长行，信息更全）
        assert!(!admit(
            &mut inner,
            welcome(WelcomeKind::Enter, 8),
            now + Duration::from_secs(1)
        ));
    }
}
