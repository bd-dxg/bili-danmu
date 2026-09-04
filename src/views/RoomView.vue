<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { RoomStatusEvent, DanmakuEvent } from "../types/ipc";
import { refreshLogin, useLogin } from "../composables/useLogin";

const { loggedIn, uid, openLoginDialog, logout } = useLogin();

const roomId = ref<number | null>(null);
const busy = ref(false); // 连接/断开操作中
const status = ref<RoomStatusEvent>({ state: "disconnected" });
const errorMsg = ref("");
/** 弹幕预览（M2 验证用，M4 由 Overlay 渲染替代） */
const danmakuList = ref<DanmakuEvent[]>([]);

const MAX_PREVIEW = 50;

let unlistenStatus: UnlistenFn | undefined;
let unlistenDanmaku: UnlistenFn | undefined;

const statusText: Record<string, string> = {
  disconnected: "未连接",
  connecting: "连接中…",
  connected: "已连接",
  reconnecting: "正在重连",
  error: "连接失败",
};

function pushDanmaku(d: DanmakuEvent) {
  danmakuList.value.push(d);
  if (danmakuList.value.length > MAX_PREVIEW) {
    danmakuList.value.splice(0, danmakuList.value.length - MAX_PREVIEW);
  }
}

async function connect() {
  const id = roomId.value;
  if (id === null || !Number.isInteger(id) || id <= 0) {
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
    danmakuList.value = [];
  } catch (e) {
    errorMsg.value = String(e);
  } finally {
    busy.value = false;
  }
}

onMounted(async () => {
  await refreshLogin();
  unlistenStatus = await listen<RoomStatusEvent>("room-status", (e) => {
    status.value = e.payload;
    if (e.payload.state === "connected") {
      errorMsg.value = "";
    }
  });
  unlistenDanmaku = await listen<DanmakuEvent>("danmaku", (e) => {
    pushDanmaku(e.payload);
  });
});

onUnmounted(() => {
  unlistenStatus?.();
  unlistenDanmaku?.();
});
</script>

<template>
  <div class="room-page">
    <div class="login-bar" :class="{ logged: loggedIn }">
      <template v-if="loggedIn">
        <span class="ok">✓ 已登录（UID {{ uid }}）</span>
        <button class="link-btn" @click="logout()">退出登录</button>
      </template>
      <template v-else>
        <span class="warn">⚠ 未登录，B 站需要登录才能接收弹幕</span>
        <button class="link-btn primary" @click="openLoginDialog()">扫码登录</button>
      </template>
    </div>

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
        房间 {{ status.roomId }}（30s 心跳保活中）
      </span>
      <span v-if="status.state === 'error' && status.message" class="room-tag">
        {{ status.message }}
      </span>
    </div>

    <div class="danmaku-preview">
      <h3>实时弹幕预览 <span class="count">{{ danmakuList.length }}</span></h3>
      <div v-if="danmakuList.length === 0" class="empty">暂无弹幕，连接直播间后自动显示</div>
      <ul class="danmaku-list">
        <li v-for="d in danmakuList" :key="d.id" class="danmaku-item">
          <span class="user">{{ d.username }}</span>
          <span class="content" :style="d.color ? { color: d.color } : {}">：{{ d.content }}</span>
        </li>
      </ul>
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

.danmaku-preview {
  margin-top: 26px;
  border-top: 1px solid var(--border);
  padding-top: 14px;
}

.danmaku-preview h3 {
  font-size: 14px;
  margin-bottom: 10px;
  color: var(--text);
}

.danmaku-preview .count {
  color: var(--text-faint);
  font-size: 12px;
  font-weight: normal;
}

.empty {
  color: var(--text-faint);
  font-size: 13px;
  padding: 12px 0;
}

.danmaku-list {
  list-style: none;
  max-height: 340px;
  overflow-y: auto;
}

.danmaku-item {
  padding: 3px 0;
  font-size: 13px;
  line-height: 1.5;
  word-break: break-all;
}

.danmaku-item .user {
  font-weight: 600;
  color: var(--text-faint);
}

.danmaku-item .content {
  color: var(--text);
}
</style>
