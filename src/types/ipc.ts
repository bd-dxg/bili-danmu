// Rust ↔ Vue IPC 共享类型定义
// 与 src-tauri 中 emit / invoke 的 payload 保持同步

/** 连接状态事件（Rust → Vue，事件名 room-status） */
export type RoomStatusEvent =
  | { state: "disconnected" }
  | { state: "connecting"; roomId: number }
  | { state: "connected"; roomId: number }
  | { state: "error"; roomId?: number; message: string }
  | { state: "reconnecting"; roomId: number; attempt: number };

/** 心跳事件（Rust → Vue，事件名 heartbeat，M1 用于验证后台 emit 链路） */
export interface HeartbeatEvent {
  ts: number;
}

/** 弹幕事件（Rust → Vue，事件名 danmaku，M2 接入） */
export interface DanmakuEvent {
  id: string;
  username: string;
  content: string;
  timestamp: number;
  color?: string;
}

/** connect_room 返回值 */
export type ConnectResult = { ok: true } | { ok: false; message: string };
