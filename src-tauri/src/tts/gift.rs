//! 礼物朗读：连击聚合与静默窗口判定
//!
//! 与 `gift` 模块（礼物**列表**渲染）刻意各算一遍，不共用状态：
//! 渲染侧只在过渲染门槛时才产出累计行，朗读门槛与它独立（可以更低甚至不同开关），
//! 拿不到未过渲染门槛的累计金额，所以朗读自己按「用户 + 礼物」分组累加。
//!
//! 时机：连击窗口内不响（否则连送 20 个就念 20 遍），窗口结束后念一次汇总，
//! 数量与金额都是累计值，门槛也按累计后的总额判定。醒目留言与上舰不连击，即时朗读。

use crate::bilibili::event::{Backing, BackingKind};
use crate::config::GiftTtsConfig;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// 进行中的连击分组
struct Pending {
    /// 累计后的行（数量与金额都已累加）
    row: Backing,
    /// 最近一次收到该分组事件的时间（静默窗口的起点）
    last_at: Instant,
}

/// 礼物朗读聚合器：纯数据 + 纯逻辑，不碰队列与定时器，便于单测
#[derive(Default)]
pub(super) struct GiftAggregator {
    groups: HashMap<String, Pending>,
}

impl GiftAggregator {
    /// 收一条打赏：连击类攒着等窗口，醒目留言与上舰返回待读行（即时朗读）
    ///
    /// 连击信号（`COMBO_SEND`）无金额也无数量，只表示连击仍在继续——
    /// 它与 `SEND_GIFT` 同时下发，后者已经刷新了窗口，这里直接忽略。
    pub(super) fn on_backing(&mut self, b: Backing, now: Instant) -> Option<Backing> {
        if b.kind == BackingKind::Combo {
            return None;
        }
        if b.kind != BackingKind::Gift {
            return Some(b);
        }
        let key = format!("{}-{}", b.uid, b.gift_id);
        match self.groups.get_mut(&key) {
            Some(p) => {
                p.last_at = now;
                p.row.num = p.row.num.saturating_add(b.num);
                p.row.amount_fen = p.row.amount_fen.saturating_add(b.amount_fen);
                None
            }
            None => {
                self.groups.insert(
                    key,
                    Pending {
                        row: b,
                        last_at: now,
                    },
                );
                None
            }
        }
    }

    /// 取出静默已满一个窗口的分组（连击结束），默认门槛没过的直接丢弃
    pub(super) fn take_due(
        &mut self,
        now: Instant,
        window: Duration,
        cfg: &GiftTtsConfig,
    ) -> Vec<Backing> {
        let threshold = threshold_fen(cfg);
        let mut due = Vec::new();
        self.groups.retain(|_, p| {
            if now.duration_since(p.last_at) < window {
                return true;
            }
            if p.row.amount_fen >= threshold {
                due.push(p.row.clone());
            }
            false
        });
        due
    }

    /// 清空未朗读的分组（关闭礼物朗读时调用）
    pub(super) fn clear(&mut self) {
        self.groups.clear();
    }
}

/// 朗读金额门槛 → 分（门槛以人民币元配置）
pub(super) fn threshold_fen(cfg: &GiftTtsConfig) -> u64 {
    (cfg.min_amount_yuan.max(0.0) * 100.0).round() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backing(kind: BackingKind, uid: i64, gift_id: i64, amount_fen: u64, num: u32) -> Backing {
        Backing {
            id: format!("{uid}-{gift_id}-{amount_fen}"),
            kind,
            uid,
            username: "老板A".into(),
            gift_name: "辣条".into(),
            gift_id,
            num,
            amount_fen,
            timestamp: 1000,
            message: None,
            medal_level: None,
            medal_name: None,
            medal_room_id: None,
            guard_level: None,
            wealth_level: None,
        }
    }

    fn cfg() -> GiftTtsConfig {
        GiftTtsConfig::default()
    }

    fn window() -> Duration {
        Duration::from_secs(5)
    }

    #[test]
    fn 连击在窗口内不朗读窗口结束后汇总一次() {
        let mut agg = GiftAggregator::default();
        let now = Instant::now();
        // 连送三次：一次都不该立刻出声
        assert!(agg
            .on_backing(backing(BackingKind::Gift, 1, 100, 100, 1), now)
            .is_none());
        assert!(agg
            .on_backing(backing(BackingKind::Gift, 1, 100, 100, 1), now)
            .is_none());
        assert!(agg
            .on_backing(backing(BackingKind::Gift, 1, 100, 100, 1), now)
            .is_none());

        // 窗口未满 → 还不念
        assert!(agg.take_due(now + Duration::from_secs(4), window(), &cfg()).is_empty());
        // 窗口满了 → 念一次，数量与金额都是累计值
        let due = agg.take_due(now + window(), window(), &cfg());
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].num, 3);
        assert_eq!(due[0].amount_fen, 300);
        // 取走后不再重复
        assert!(agg.take_due(now + window(), window(), &cfg()).is_empty());
    }

    #[test]
    fn 同一观众不同礼物各自成组() {
        let mut agg = GiftAggregator::default();
        let now = Instant::now();
        agg.on_backing(backing(BackingKind::Gift, 1, 100, 100, 1), now);
        agg.on_backing(backing(BackingKind::Gift, 1, 200, 500, 1), now);
        agg.on_backing(backing(BackingKind::Gift, 2, 100, 100, 1), now);
        let due = agg.take_due(now + window(), window(), &cfg());
        assert_eq!(due.len(), 3, "三种「用户 + 礼物」组合应各念一次");
    }

    #[test]
    fn 门槛按累计后的总额判定() {
        let mut cfg = cfg();
        cfg.min_amount_yuan = 30.0; // 3000 分

        // 连送 29 个 1 元：累计 2900 分未过门槛，静默后直接丢弃
        let mut agg = GiftAggregator::default();
        let now = Instant::now();
        for _ in 0..29 {
            agg.on_backing(backing(BackingKind::Gift, 1, 100, 100, 1), now);
        }
        assert!(agg.take_due(now + window(), window(), &cfg).is_empty());

        // 连送 30 个 1 元：累计到 3000 分应过门槛
        let mut agg = GiftAggregator::default();
        for _ in 0..30 {
            agg.on_backing(backing(BackingKind::Gift, 1, 100, 100, 1), now);
        }
        let due = agg.take_due(now + window(), window(), &cfg);
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].amount_fen, 3000);
        assert_eq!(due[0].num, 30);
    }

    #[test]
    fn 醒目留言与上舰即时朗读不聚合() {
        let mut agg = GiftAggregator::default();
        let now = Instant::now();
        let sc = agg
            .on_backing(backing(BackingKind::SuperChat, 1, 0, 3000, 1), now)
            .expect("醒目留言应立刻返回待读行");
        assert_eq!(sc.kind, BackingKind::SuperChat);
        let guard = agg
            .on_backing(backing(BackingKind::Guard, 1, 0, 138000, 1), now)
            .expect("上舰应立刻返回待读行");
        assert_eq!(guard.kind, BackingKind::Guard);
        // 即时朗读的不进分组表，窗口结束后没有汇总
        assert!(agg.take_due(now + window(), window(), &cfg()).is_empty());
    }

    #[test]
    fn 连击信号被忽略() {
        let mut agg = GiftAggregator::default();
        let now = Instant::now();
        assert!(agg
            .on_backing(backing(BackingKind::Combo, 1, 100, 0, 0), now)
            .is_none());
        assert!(agg.take_due(now + window(), window(), &cfg()).is_empty());
    }

    #[test]
    fn 窗口内再次送达会延长静默判定() {
        let mut agg = GiftAggregator::default();
        let now = Instant::now();
        agg.on_backing(backing(BackingKind::Gift, 1, 100, 100, 1), now);
        // 第 4 秒又来一个：静默重新计时，第 5 秒还不该念
        agg.on_backing(
            backing(BackingKind::Gift, 1, 100, 100, 1),
            now + Duration::from_secs(4),
        );
        assert!(agg
            .take_due(now + window(), window(), &cfg())
            .is_empty());
        let due = agg.take_due(now + Duration::from_secs(9), window(), &cfg());
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].num, 2);
    }

    #[test]
    fn 清空后不再朗读旧分组() {
        let mut agg = GiftAggregator::default();
        let now = Instant::now();
        agg.on_backing(backing(BackingKind::Gift, 1, 100, 100, 1), now);
        agg.clear();
        assert!(agg.take_due(now + window(), window(), &cfg()).is_empty());
    }
}
