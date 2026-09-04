// Rust ↔ Vue IPC 共享类型定义
// 与 src-tauri 中 emit / invoke 的 payload 保持同步

/** 连接状态事件（Rust → Vue，事件名 room-status） */
export type RoomStatusEvent =
  | { state: "disconnected" }
  | { state: "connecting"; roomId: number }
  /** roomId 为真实房间号（短号解析后） */
  | { state: "connected"; roomId: number }
  | { state: "error"; roomId?: number; message: string }
  | { state: "reconnecting"; roomId: number; attempt: number };

/** 弹幕事件（Rust → Vue，事件名 danmaku） */
export interface DanmakuEvent {
  id: string;
  username: string;
  content: string;
  /** Unix 秒 */
  timestamp: number;
  /** 弹幕颜色，如 "#FFFFFF"；缺省用默认色 */
  color?: string;
}

/** connect_room 返回值 */
export type ConnectResult = { ok: true } | { ok: false; message: string };
