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

/// 解析出的 B 站事件
///
/// V1 只消费 Danmaku；其余命令（礼物/进场/SC 等）在 parser 层直接忽略，
/// 避免高频事件流造成日志洪泛与无谓分配
#[derive(Debug, Clone)]
pub enum BilibiliEvent {
    Danmaku(Danmaku),
}
