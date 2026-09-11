//! B 站直播弹幕事件模型

use serde::Serialize;

/// 弹幕消息（V1 核心数据结构）
#[derive(Debug, Clone, Serialize)]
pub struct Danmaku {
    pub id: String,
    pub username: String,
    pub content: String,
    /// 发送时间（Unix 秒）
    pub timestamp: i64,
    /// 弹幕颜色（如 "#FFFFFF"），缺省时前端用默认色
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    // ---- 用户身份（M3.5 增强，供 TTS 过滤条件与名牌展示） ----
    /// 粉丝勋章等级（0/None = 未佩戴）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medal_level: Option<u32>,
    /// 粉丝勋章名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medal_name: Option<String>,
    /// 勋章所属房间 ID（等于当前直播间 ID = 主播自己的牌子）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medal_room_id: Option<u32>,
    /// 用户等级（1-40+）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_level: Option<u32>,
    /// 舰队等级：0 无 / 3 舰长 / 2 提督 / 1 总督
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guard_level: Option<u32>,
    /// 荣耀等级（全站财富等级，直播页面常见展示）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wealth_level: Option<u32>,
    /// 是否房管
    #[serde(default)]
    pub is_admin: bool,
}

/// 打赏形态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BackingKind {
    /// 普通礼物（SEND_GIFT）
    Gift,
    /// 连击信号（COMBO_SEND）：只表示连击仍在继续，数量与金额一律以 SEND_GIFT 为准
    Combo,
    /// 醒目留言 / SC（SUPER_CHAT_MESSAGE）
    SuperChat,
    /// 上舰（GUARD_BUY）
    Guard,
}

/// 打赏消息（礼物 / 连击 / 醒目留言 / 上舰）
///
/// 金额统一折算成整数「分」（`amount_fen`），避免浮点误差：
/// 金瓜子 1000 = 1 元 = 100 分，即 10 金瓜子 = 1 分；醒目留言与上舰的原始单位
/// 分别是人民币元与金瓜子单价，在 parser 层统一折成同一口径。
/// 免费礼物（银瓜子）不会产生此事件。
#[derive(Debug, Clone, Serialize)]
pub struct Backing {
    /// 消息去重标识（礼物优先用服务端 rnd，缺失时按时间戳 + 用户 + 礼物拼）
    pub id: String,
    pub kind: BackingKind,
    pub uid: i64,
    pub username: String,
    /// 礼物名 / 上舰档位名（舰长 / 提督 / 总督）/ 醒目留言固定为「醒目留言」
    pub gift_name: String,
    /// 礼物 ID（连击合并按「用户 + 礼物」分组用）
    pub gift_id: i64,
    pub num: u32,
    /// 人民币价值（分）
    pub amount_fen: u64,
    /// 发送时间（Unix 秒）
    pub timestamp: i64,
    /// 醒目留言正文
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    // ---- 用户身份（与 Danmaku 同构，供礼物行复用弹幕的名牌列） ----
    /// 粉丝勋章等级（0/None = 未佩戴）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medal_level: Option<u32>,
    /// 粉丝勋章名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medal_name: Option<String>,
    /// 勋章所属房间 ID（等于当前直播间 ID = 主播自己的牌子）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medal_room_id: Option<u32>,
    /// 舰队等级：0 无 / 3 舰长 / 2 提督 / 1 总督
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guard_level: Option<u32>,
    /// 荣耀等级（全站财富等级）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wealth_level: Option<u32>,
}

/// 解析出的 B 站事件
///
/// 未列入的命令（进场/通知等）在 parser 层直接忽略，
/// 避免高频事件流造成日志洪泛与无谓分配
#[derive(Debug, Clone)]
pub enum BilibiliEvent {
    Danmaku(Danmaku),
    Backing(Backing),
}
