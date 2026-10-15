<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { onMounted, ref, watch } from 'vue'

import StatusDashboard from '../components/StatusDashboard.vue'
import { refreshLogin, useLogin } from '../composables/useLogin'
import { useRoomConnection } from '../composables/useRoomConnection'

import type { FollowingLive } from '../types/ipc'

onMounted(refreshLogin)

const { loggedIn, uid, uname, openLoginDialog, logout } = useLogin()
const { roomId, busy, status, errorMsg, recentRooms, locked, statusText, connect, disconnect } = useRoomConnection()

// 「关注在播」：只在点刷新时查（两个接口，不随页面挂载自动拉）
const following = ref<FollowingLive[]>([])
const followingBusy = ref(false)
const followingErr = ref('')
const followingLoaded = ref(false)

async function loadFollowingLive() {
  followingBusy.value = true
  followingErr.value = ''
  try {
    const list = await invoke<FollowingLive[]>('list_following_live')
    // 刷新期间退出登录：丢弃晚到的结果，避免登出后又冒出列表
    if (!loggedIn.value) return
    following.value = list
    followingLoaded.value = true
  } catch (e) {
    if (!loggedIn.value) return
    followingErr.value = String(e)
  } finally {
    followingBusy.value = false
  }
}

// 「关注在播」：已登录时自动查一次（打开软件已是登录态 / 扫码登录成功都走这条），
// 之后追加查询走「刷新」按钮
watch(loggedIn, on => {
  if (on) void loadFollowingLive()
})

// 最近连接的直播间面包屑：暂时隐藏（只隐藏 UI，数据与代码保留，改 true 即恢复）
const SHOW_RECENT_ROOMS = false

// 退出登录：先断开直播连接，再清登录态（登录是收弹幕的前提）
async function handleLogout() {
  if (status.value.state === 'connected' || status.value.state === 'connecting') {
    await disconnect()
  }
  await logout()
  // 关注在播是登录态数据，退出后清掉并回到「未刷新」
  following.value = []
  followingLoaded.value = false
  followingErr.value = ''
}
</script>

<template>
  <div class="room-page">
    <div class="login-bar" :class="{ logged: loggedIn }">
      <template v-if="loggedIn">
        <span class="ok">
          ✓ 已登录
          <span v-if="uname">：{{ uname }}</span>
          <span v-else>（UID {{ uid }}）</span>
        </span>
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
        :disabled="locked" />
      <button v-if="status.state === 'connected'" class="btn danger" :disabled="busy" @click="disconnect">断开</button>
      <button v-else class="btn primary" :disabled="busy || status.state === 'connecting'" @click="connect()">
        连接
      </button>
    </div>

    <div v-if="SHOW_RECENT_ROOMS && recentRooms.length" class="recent-bar">
      <span class="recent-label">最近</span>
      <button
        v-for="r in recentRooms"
        :key="r.room_id"
        class="crumb"
        :title="`房间 ${r.room_id}`"
        :disabled="busy || locked"
        @click="connect(r.room_id)">
        {{ r.uname || r.room_id }}
      </button>
    </div>

    <div v-if="loggedIn" class="following-bar">
      <span class="recent-label">关注在播</span>
      <button class="link-btn" :disabled="followingBusy || !loggedIn" @click="loadFollowingLive">
        {{ followingBusy ? '查询中…' : '刷新' }}
      </button>
      <span v-if="followingErr" class="error">{{ followingErr }}</span>
      <span v-else-if="followingLoaded && !following.length" class="recent-label">（当前没有关注的主播在播）</span>
      <button
        v-for="f in following"
        :key="f.uid"
        class="crumb"
        :title="`房间 ${f.room_id}`"
        :disabled="busy || locked"
        @click="connect(f.room_id)">
        {{ f.uname }}
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
        }"></span>
      <span>{{ statusText }}</span>
      <span v-if="status.state === 'connected'" class="room-tag">房间 {{ status.roomId }}（30s 心跳保活中）</span>
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

/* 关注在播：与「最近」同形态（都是可点直连的名字行），仅上方多一条分隔线 */
.following-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 2px 8px;
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px solid var(--border);
}

.following-bar .error {
  margin: 0;
  font-size: 14px;
}

/* 该区第一个名字紧邻刷新按钮（前面不是 .crumb），分隔符单独补一条 */
.following-bar .crumb::before {
  content: '›';
  color: var(--text-faint);
  margin-right: 8px;
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
  content: '›';
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
