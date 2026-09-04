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
}

/// 解析出的 B 站事件
///
/// V1 只消费 Danmaku；其他事件记录日志但不对外发送
#[derive(Debug, Clone)]
pub enum BilibiliEvent {
    Danmaku(Danmaku),
    /// 其他命令（礼物/进场/SC 等），仅记录
    Other(String),
}
