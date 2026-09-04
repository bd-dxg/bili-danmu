<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { RoomStatusEvent } from "../types/ipc";
import { refreshLogin, useLogin } from "../composables/useLogin";

const { loggedIn, uid, openLoginDialog, logout } = useLogin();
const roomId = ref("");
const busy = ref(false); // 连接/断开操作中
const status = ref<RoomStatusEvent>({ state: "disconnected" });
const errorMsg = ref("");

let unlistenStatus: UnlistenFn | undefined;

const statusText: Record<string, string> = {
  disconnected: "未连接",
  connecting: "连接中…",
  connected: "已连接",
  reconnecting: "正在重连",
  error: "连接失败",
};

async function connect() {
  const id = Number(roomId.value);
  if (!Number.isInteger(id) || id <= 0) {
    errorMsg.value = "请输入有效的直播间 ID";
    return;
  }
  errorMsg.value = "";
  busy.value = true;
  try {
    await invoke("connect_room", { roomId: id });
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

// 退出登录：先断开直播连接，再清登录态（登录是收弹幕的前提）
async function handleLogout() {
  if (status.value.state === "connected" || status.value.state === "connecting") {
    await disconnect();
  }
  await logout();
}

onMounted(async () => {
  await refreshLogin();
  unlistenStatus = await listen<RoomStatusEvent>("room-status", (e) => {
    status.value = e.payload;
    if (e.payload.state === "connected") {
      errorMsg.value = "";
    }
  });
});

onUnmounted(() => {
  unlistenStatus?.();
});
</script>

<template>
  <div class="room-page">
    <div class="login-bar" :class="{ logged: loggedIn }">
      <template v-if="loggedIn">
        <span class="ok">✓ 已登录（UID {{ uid }}）</span>
        <button class="link-btn" @click="handleLogout">退出登录</button>
      </template>
      <template v-else>
        <span class="warn">⚠ 未登录，B 站需要登录才能接收弹幕</span>
        <button class="link-btn primary" @click="openLoginDialog()">扫码登录</button>
      </template>
    </div>

    <h2>直播间</h2>

    <div class="row">
      <input
        v-model="roomId"
        class="room-input"
        type="number"
        placeholder="输入直播间 ID，如 22312451"
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
        房间 {{ status.roomId }}（30s 心跳保活中）
      </span>
      <span v-if="status.state === 'error' && status.message" class="room-tag">
        {{ status.message }}
      </span>
    </div>
  </div>
</template>

<style scoped>
.room-page {
  display: flex;
  flex-direction: column;
}

.login-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  background: var(--bg-side);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 6px 10px;
  margin-bottom: 16px;
  font-size: 13px;
}

.login-bar .ok {
  color: var(--green);
}

.login-bar .warn {
  color: var(--yellow);
}

.link-btn {
  background: none;
  border: none;
  color: var(--accent);
  font-size: 13px;
  cursor: pointer;
  padding: 2px 6px;
  border-radius: 4px;
}

.link-btn:hover {
  background: var(--hover);
}

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
</style>
