//! 配置结构体与默认值（纯数据定义，不含任何读写逻辑）
//!
//! 字段随里程碑扩展：M2 登录 Cookie；M4 Overlay 弹幕样式；M5 朗读；M6 完整应用配置。

use serde::{Deserialize, Serialize};

/// B 站登录态（游客为空）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthInfo {
    /// 登录用户 UID（0 表示游客）
    pub uid: i64,
    /// 登录 Cookie 串（如 "SESSDATA=..; DedeUserID=.."）——内存保持明文供 HTTP 请求使用；
    /// 落盘时经 DPAPI 加密（见 crypto::encrypt_cookie），从磁盘读取后自动解密
    pub cookies: String,
    /// 用户昵称（nav 接口获取；旧配置无此字段为 None）
    #[serde(default)]
    pub uname: Option<String>,
}

/// Overlay 弹幕样式（M4）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayStyle {
    /// 弹幕字号（px）
    pub font_size: f64,
    /// 字体族
    pub font_family: String,
    /// 是否显示荣耀等级徽章
    pub show_wealth: bool,
    /// 是否显示粉丝牌
    pub show_medal: bool,
    /// 是否显示舰长/房管身份前缀
    pub show_role: bool,
    /// 用户名颜色（#RRGGBB）
    pub username_color: String,
    /// 弹幕内容颜色（#RRGGBB）
    pub content_color: String,
    /// 文字加粗
    pub bold: bool,
    /// 是否文字描边
    pub outline: bool,
    /// 描边颜色（#RRGGBB）
    pub outline_color: String,
    /// 描边宽度（px 近似值）
    pub outline_width: f64,
    /// 弹幕行间距（px，0=不额外加，仅行高）
    pub row_gap: f64,
    /// 弹幕区 / 礼物区的背景不透明度（0-100，0 = 完全透明，100 = 纯黑）
    pub bg_opacity: f64,
}

impl Default for OverlayStyle {
    fn default() -> Self {
        Self {
            font_size: 17.0,
            font_family: "Microsoft YaHei UI".into(),
            show_wealth: true,
            show_medal: true,
            show_role: true,
            username_color: "#85DEF1".into(),
            content_color: "#FFFFFF".into(),
            bold: true,
            outline: true,
            outline_color: "#000000".into(),
            outline_width: 2.0,
            row_gap: 0.0,
            bg_opacity: 35.0,
        }
    }
}

/// Overlay 窗口位置大小（M3.5，逻辑坐标）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// 弹幕过滤配置
///
/// 身份规则间为「或」关系：任一开启的规则命中即显示（全部关闭 = 不过滤）；
/// 敏感词屏蔽独立叠加：开关开启且命中词表时整条丢弃，白名单规则不豁免。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DanmakuFilter {
    /// 只显示舰长（全部大航海）/ 房管弹幕
    pub enable_guard_admin: bool,
    /// 只显示有粉丝牌的弹幕（B 站实际只下发当前房间粉丝牌）
    pub enable_medal: bool,
    /// 只显示荣耀等级 ≥ wealth_min 的弹幕
    pub enable_wealth: bool,
    /// 荣耀等级门槛（配合 enable_wealth）
    pub wealth_min: u32,
    /// 屏蔽命中敏感词的弹幕（整条丢弃）
    pub enable_sensitive: bool,
    /// 敏感词表（弹幕内容含任一即丢弃）
    pub sensitive_words: Vec<String>,
}

impl Default for DanmakuFilter {
    fn default() -> Self {
        Self {
            enable_guard_admin: false,
            enable_medal: false,
            enable_wealth: false,
            wealth_min: 0,
            enable_sensitive: false,
            sensitive_words: Vec::new(),
        }
    }
}

/// TTS 弹幕朗读配置（M5）
///
/// 朗读筛选用独立的 `DanmakuFilter` 实例：显示全开、只朗读舰长这类组合才成立。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TtsConfig {
    /// 总开关
    pub enabled: bool,
    /// Edge TTS 音色名（如 zh-CN-XiaoxiaoNeural）
    pub voice: String,
    /// 语速百分比偏移（-50 = 半速，+50 = 1.5 倍速）
    pub rate_pct: i32,
    /// 音量百分比偏移
    pub volume_pct: i32,
    /// 是否在正文前念用户名
    pub read_username: bool,
    /// 是否在正文前念身份前缀（房管 / 舰长）
    pub read_role: bool,
    /// 弹幕正文最大朗读字数（不含身份前缀与用户名；超出截断，0 = 不限制）
    pub max_len: u32,
    /// 待朗读队列上限（超出丢弃最旧的）
    pub max_queue: u32,
    /// 积压时丢弃待播的旧弹幕（作废还没开播的在途音频，正在念的那条念完）
    pub interrupt_on_backlog: bool,
    /// 朗读筛选条件（与弹幕显示筛选相互独立）
    pub filter: DanmakuFilter,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            voice: "zh-CN-XiaoxiaoNeural".into(),
            rate_pct: 0,
            volume_pct: 0,
            read_username: false,
            read_role: false,
            // 15 字 ≈ 3.4s 音频；B 站弹幕上限 30–40 字（≈ 7–9s），全念完在高频房间会明显积压
            max_len: 15,
            max_queue: 5,
            interrupt_on_backlog: true,
            filter: DanmakuFilter::default(),
        }
    }
}

/// 礼物列表配置
///
/// 门槛以人民币元配置（金瓜子 1000 = 1 元），连击按累加后的总额判定。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GiftConfig {
    /// 是否在弹幕窗展示礼物区
    pub enabled: bool,
    /// 打赏金额门槛（元）：低于此价值的打赏不进礼物列表（0 = 全部付费打赏）
    pub min_amount_yuan: f64,
    /// 礼物区最多同时显示的条数
    pub max_rows: u32,
    /// 连击合并窗口（秒）：同一观众同一种礼物在该时间内的多次送出合并为一行
    pub combo_window_secs: u64,
}

impl Default for GiftConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_amount_yuan: 0.0,
            max_rows: 5,
            combo_window_secs: 5,
        }
    }
}

/// 最近连接过的直播间（主界面输入框下方面包屑，点击直连）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentRoom {
    /// 真实房间号（短号已由解析接口还原）
    pub room_id: u32,
    /// 主播昵称（接口获取失败为 None，前端回退显示房间号）
    #[serde(default)]
    pub uname: Option<String>,
}

/// 最近房间保留条数上限
pub const RECENT_ROOM_LIMIT: usize = 10;

/// 配置文件结构（后续里程碑扩展字段）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigFile {
    pub auth: Option<AuthInfo>,
    pub overlay_style: OverlayStyle,
    pub overlay_bounds: Option<WindowBounds>,
    pub danmaku_filter: DanmakuFilter,
    pub tts: TtsConfig,
    pub gift: GiftConfig,
    pub recent_rooms: Vec<RecentRoom>,
}
