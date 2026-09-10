<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { RecentRoom, RoomStatusEvent } from "../types/ipc";
import { refreshLogin, useLogin } from "../composables/useLogin";
import StatusDashboard from "../components/StatusDashboard.vue";

const { loggedIn, uid, uname, openLoginDialog, logout } = useLogin();
const roomId = ref("");
const busy = ref(false); // 连接/断开操作中
const status = ref<RoomStatusEvent>({ state: "disconnected" });
const errorMsg = ref("");
// 最近连接过的直播间（Rust 持久化，输入框下方面包屑）
const recentRooms = ref<RecentRoom[]>([]);

let unlistenStatus: UnlistenFn | undefined;
let unlistenRecent: UnlistenFn | undefined;

// 连接中/已连接时锁定房间号输入框与最近面包屑：改号需先断开，避免输入框与实际连接目标不一致
const locked = computed(
  () => status.value.state === "connecting" || status.value.state === "connected",
);

const statusText: Record<string, string> = {
  disconnected: "未连接",
  connecting: "连接中…",
  connected: "已连接",
  error: "连接失败",
};

// target 为面包屑直连的房间号：回填输入框，保证输入框与连接目标一致
async function connect(target?: number) {
  if (target !== undefined) {
    roomId.value = String(target);
  }
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

// 读取最近房间（连接成功后 Rust 侧已写入，含主播名）
async function loadRecentRooms() {
  try {
    recentRooms.value = await invoke<RecentRoom[]>("get_recent_rooms");
  } catch {
    // 读取失败不影响连接功能，保持空列表
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
  await loadRecentRooms();
  unlistenStatus = await listen<RoomStatusEvent>("room-status", (e) => {
    status.value = e.payload;
    if (e.payload.state === "connected") {
      errorMsg.value = "";
    }
  });
  // 最近房间由 Rust 后台任务抽取（与连接并行，不阻塞弹幕会话），写完广播一次
  unlistenRecent = await listen("recent-rooms-changed", () => {
    void loadRecentRooms();
  });
});

onUnmounted(() => {
  unlistenStatus?.();
  unlistenRecent?.();
});
</script>

<template>
  <div class="room-page">
    <div class="login-bar" :class="{ logged: loggedIn }">
      <template v-if="loggedIn">
        <span class="ok">✓ 已登录<span v-if="uname">：{{ uname }}</span><span v-else>（UID {{ uid }}）</span></span>
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
        :disabled="locked"
      />
      <button
        v-if="status.state === 'connected'"
        class="btn danger"
        :disabled="busy"
        @click="disconnect"
      >
        断开
      </button>
      <button
        v-else
        class="btn primary"
        :disabled="busy || status.state === 'connecting'"
        @click="connect()"
      >
        连接
      </button>
    </div>

    <div v-if="recentRooms.length" class="recent-bar">
      <span class="recent-label">最近</span>
      <button
        v-for="r in recentRooms"
        :key="r.room_id"
        class="crumb"
        :title="`房间 ${r.room_id}`"
        :disabled="busy || locked"
        @click="connect(r.room_id)"
      >
        {{ r.uname || r.room_id }}
      </button>
    </div>

    <p v-if="errorMsg" class="error">{{ errorMsg }}</p>

    <div class="status-box">
      <span
        class="dot"
        :class="{
          green: status.state === 'connected',
          yellow: status.state === 'connecting',
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

    <StatusDashboard />
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
  font-size: 15px;
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
  font-size: 15px;
  cursor: pointer;
  padding: 2px 6px;
  border-radius: 4px;
}

.link-btn:hover {
  background: var(--hover);
}

h2 {
  font-size: 18px;
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
  font-size: 16px;
  outline: none;
}

.room-input:focus {
  border-color: var(--accent);
}

/* 锁定态与按钮禁用观感统一 */
.room-input:disabled {
  color: var(--text-faint);
  cursor: not-allowed;
  opacity: 0.7;
}

/* 隐藏 number 输入框自带的上下步进箭头 */
.room-input::-webkit-outer-spin-button,
.room-input::-webkit-inner-spin-button {
  -webkit-appearance: none;
  margin: 0;
}

.btn {
  border: none;
  border-radius: 6px;
  padding: 8px 18px;
  font-size: 16px;
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
  font-size: 15px;
  margin-top: 8px;
}

/* 最近房间面包屑：点击直连；连接中/已连接时禁用（需先断开） */
.recent-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 2px 8px;
  margin-top: 10px;
}

.recent-label {
  color: var(--text-faint);
  font-size: 14px;
}

.crumb {
  background: none;
  border: none;
  padding: 2px 0;
  color: var(--accent);
  font-size: 14px;
  cursor: pointer;
  max-width: 12em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.crumb + .crumb::before {
  content: "›";
  color: var(--text-faint);
  margin-right: 8px;
}

.crumb:hover:not(:disabled) {
  text-decoration: underline;
}

.crumb:disabled {
  color: var(--text-faint);
  cursor: not-allowed;
}

.status-box {
  margin-top: 18px;
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 16px;
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
  font-size: 14px;
}
</style>
