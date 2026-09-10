// Rust ↔ Vue IPC 共享类型定义
// 与 src-tauri 中 emit / invoke 的 payload 保持同步

/** 连接状态事件（Rust → Vue，事件名 room-status） */
export type RoomStatusEvent =
  | { state: "disconnected" }
  | { state: "connecting"; roomId: number }
  /** roomId 为真实房间号（短号解析后） */
  | { state: "connected"; roomId: number }
  | { state: "error"; roomId?: number; message: string };

/** 弹幕事件（Rust → Vue，事件名 danmaku） */
export interface DanmakuEvent {
  id: string;
  username: string;
  content: string;
  /** Unix 秒 */
  timestamp: number;
  /** 弹幕颜色，如 "#FFFFFF"；缺省用默认色 */
  color?: string;
  // ---- 用户身份（M3.5 增强，供 TTS 过滤与名牌展示） ----
  /** 粉丝勋章等级 */
  medal_level?: number;
  /** 粉丝勋章名 */
  medal_name?: string;
  /** 勋章所属房间 ID（= 当前房间即主播粉丝牌） */
  medal_room_id?: number;
  /** 用户等级 */
  user_level?: number;
  /** 舰队：3舰长 2提督 1总督 */
  guard_level?: number;
  /** 荣耀等级（全站财富等级） */
  wealth_level?: number;
  /** 是否房管 */
  is_admin: boolean;
}

/** Overlay 弹幕样式（Rust ↔ Vue） */
export interface OverlayStyle {
  font_size: number;
  font_family: string;
  show_wealth: boolean;
  show_medal: boolean;
  show_role: boolean;
  username_color: string;
  content_color: string;
  bold: boolean;
  outline: boolean;
  outline_color: string;
  outline_width: number;
  /** 弹幕行间距 px（0=不额外加，仅行高） */
  row_gap: number;
}

/** connect_room 返回值 */
export type ConnectResult = { ok: true } | { ok: false; message: string };

/** 最近连接过的直播间（Rust 持久化，主界面输入框下方面包屑） */
export interface RecentRoom {
  /** 真实房间号（短号已解析） */
  room_id: number;
  /** 主播昵称（接口获取失败为 null，前端回退显示房间号） */
  uname?: string | null;
}

/** 弹幕过滤配置（Rust ↔ Vue，事件名 danmaku-filter） */
export interface DanmakuFilter {
  /** 只显示舰长（全部大航海）/ 房管弹幕 */
  enable_guard_admin: boolean;
  /** 只显示有粉丝牌的弹幕 */
  enable_medal: boolean;
  /** 只显示荣耀等级 ≥ wealth_min 的弹幕 */
  enable_wealth: boolean;
  /** 荣耀等级门槛 */
  wealth_min: number;
  /** 屏蔽含敏感词的弹幕（整条丢弃） */
  enable_sensitive: boolean;
  /** 敏感词表（内容含任一即丢弃） */
  sensitive_words: string[];
}

/** 弹幕过滤默认值（全关 = 不过滤） */
export const DEFAULT_DANMAKU_FILTER: DanmakuFilter = {
  enable_guard_admin: false,
  enable_medal: false,
  enable_wealth: false,
  wealth_min: 0,
  enable_sensitive: false,
  sensitive_words: [],
};

/** Edge TTS 音色（tts_list_voices 返回） */
export interface TtsVoice {
  id: string;
  label: string;
}

/** TTS 弹幕朗读配置（Rust ↔ Vue） */
export interface TtsConfig {
  /** 总开关 */
  enabled: boolean;
  /** Edge TTS 音色名 */
  voice: string;
  /** 语速百分比偏移（-50 = 半速，+50 = 1.5 倍速） */
  rate_pct: number;
  /** 音量百分比偏移 */
  volume_pct: number;
  /** 是否在正文前念用户名 */
  read_username: boolean;
  /** 是否在正文前念身份前缀（房管 / 舰长） */
  read_role: boolean;
  /** 弹幕正文最大朗读字数（不含身份前缀与用户名，0 = 不限制） */
  max_len: number;
  /** 待朗读队列上限（超出丢弃最旧的） */
  max_queue: number;
  /** 积压时打断当前朗读 */
  interrupt_on_backlog: boolean;
  /** 朗读筛选条件（与弹幕显示筛选相互独立） */
  filter: DanmakuFilter;
}

/** TTS 默认配置 */
export const DEFAULT_TTS_CONFIG: TtsConfig = {
  enabled: false,
  voice: "zh-CN-XiaoxiaoNeural",
  rate_pct: 0,
  volume_pct: 0,
  read_username: false,
  read_role: false,
  // 15 字 ≈ 3.4s 音频；B 站弹幕上限 30–40 字（≈ 7–9s），全念完在高频房间会明显积压
  max_len: 15,
  max_queue: 5,
  interrupt_on_backlog: true,
  filter: DEFAULT_DANMAKU_FILTER,
};
