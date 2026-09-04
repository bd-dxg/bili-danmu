<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { RoomStatusEvent, HeartbeatEvent } from "../types/ipc";

const roomId = ref<number | null>(null);
const busy = ref(false); // 连接/断开操作中
const status = ref<RoomStatusEvent>({ state: "disconnected" });
const heartbeat = ref<number | null>(null);
const errorMsg = ref("");

let unlistenStatus: UnlistenFn | undefined;
let unlistenBeat: UnlistenFn | undefined;

const statusText: Record<string, string> = {
  disconnected: "未连接",
  connecting: "连接中…",
  connected: "已连接",
  reconnecting: "正在重连",
  error: "连接失败",
};

async function connect() {
  const id = roomId.value;
  if (id === null || !Number.isInteger(id) || id <= 0) {
    errorMsg.value = "请输入有效的直播间 ID";
    return;
  }
  errorMsg.value = "";
  busy.value = true;
  try {
    const res = await invoke<{ ok: boolean; message?: string }>("connect_room", {
      roomId: id,
    });
    if (!res.ok) {
      status.value = { state: "error", message: res.message ?? "未知错误" };
    }
  } catch (e) {
    status.value = { state: "error", message: String(e) };
  } finally {
    busy.value = false;
  }
}

async function disconnect() {
  busy.value = true;
  try {
    await invoke("disconnect_room");
  } catch (e) {
    errorMsg.value = String(e);
  } finally {
    busy.value = false;
  }
}

onMounted(async () => {
  unlistenStatus = await listen<RoomStatusEvent>("room-status", (e) => {
    status.value = e.payload;
  });
  unlistenBeat = await listen<HeartbeatEvent>("heartbeat", (e) => {
    heartbeat.value = e.payload.ts;
  });
});

onUnmounted(() => {
  unlistenStatus?.();
  unlistenBeat?.();
});
</script>

<template>
  <div class="room-page">
    <h2>直播间</h2>

    <div class="row">
      <input
        v-model.number="roomId"
        class="room-input"
        type="number"
        placeholder="直播间 ID，如 123456"
        :disabled="status.state === 'connected' || status.state === 'connecting'"
        @keydown.enter="connect"
      />
      <button
        v-if="status.state === 'connected'"
        class="btn danger"
        :disabled="busy"
        @click="disconnect"
      >
        断开
      </button>
      <button v-else class="btn primary" :disabled="busy" @click="connect">
        连接
      </button>
    </div>

    <p v-if="errorMsg" class="error">{{ errorMsg }}</p>

    <div class="status-box">
      <span
        class="dot"
        :class="{
          green: status.state === 'connected',
          yellow: status.state === 'connecting' || status.state === 'reconnecting',
          red: status.state === 'error',
          gray: status.state === 'disconnected',
        }"
      ></span>
      <span>{{ statusText[status.state] ?? status.state }}</span>
      <span v-if="status.state === 'connected'" class="room-tag">
        房间 {{ status.roomId }}
      </span>
      <span v-if="status.state === 'error' && status.message" class="room-tag">
        {{ status.message }}
      </span>
    </div>

    <div class="ipc-check">
      <h3>IPC 链路检查（M1）</h3>
      <p>
        最后一次 Rust 后台心跳：
        {{ heartbeat === null ? "未收到" : new Date(heartbeat * 1000).toLocaleTimeString() }}
      </p>
      <p class="hint">连接后 Rust 每秒推送心跳事件 → 证明 Rust → Vue 事件链路通畅。</p>
    </div>
  </div>
</template>

<style scoped>
h2 {
  font-size: 16px;
  margin-bottom: 14px;
}

.row {
  display: flex;
  gap: 8px;
}

.room-input {
  flex: 1;
  background: var(--bg-elev);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  padding: 8px 10px;
  font-size: 14px;
  outline: none;
}

.room-input:focus {
  border-color: var(--accent);
}

.btn {
  border: none;
  border-radius: 6px;
  padding: 8px 18px;
  font-size: 14px;
  color: #fff;
}

.btn.primary {
  background: var(--accent);
}

.btn.primary:hover {
  background: var(--accent-hover);
}

.btn.danger {
  background: var(--danger);
}

.btn.danger:hover {
  background: var(--danger-hover);
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.error {
  color: var(--red);
  font-size: 13px;
  margin-top: 8px;
}

.status-box {
  margin-top: 18px;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
}

.dot {
  width: 9px;
  height: 9px;
  border-radius: 50%;
}

.dot.green {
  background: var(--green);
}
.dot.yellow {
  background: var(--yellow);
}
.dot.red {
  background: var(--red);
}
.dot.gray {
  background: var(--gray);
}

.room-tag {
  color: var(--text-faint);
  font-size: 12px;
}

.ipc-check {
  margin-top: 26px;
  border-top: 1px solid var(--border);
  padding-top: 14px;
  font-size: 13px;
  color: var(--text-dim);
}

.ipc-check h3 {
  color: var(--text);
  font-size: 14px;
  margin-bottom: 8px;
}

.hint {
  color: var(--text-faint);
  font-size: 12px;
  margin-top: 4px;
}
</style>
