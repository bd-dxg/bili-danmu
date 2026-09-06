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
