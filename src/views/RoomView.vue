<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { onMounted, onUnmounted, ref } from 'vue'

import QuickControl from '../components/QuickControl.vue'
import { refreshLogin, useLogin } from '../composables/useLogin'
import { useRoomConnection } from '../composables/useRoomConnection'

import type { TtsConfig, GiftTtsConfig, GiftConfig, WelcomeConfig, OverlayStyle } from '../types/ipc'

onMounted(refreshLogin)

const { loggedIn, uid, uname, openLoginDialog, logout } = useLogin()
const { roomId, busy, status, errorMsg, recentRooms, locked, statusText, connect, disconnect } = useRoomConnection()

// 退出登录：先断开直播连接，再清登录态（登录是收弹幕的前提）
async function handleLogout() {
  if (status.value.state === 'connected' || status.value.state === 'connecting') {
    await disconnect()
  }
  await logout()
}

// 快捷控制相关状态
const overlayVisible = ref(true)
const overlayClickthrough = ref(true)
const overlayAlwaysOnTop = ref(true)
const ttsEnabled = ref(false)
const giftTtsEnabled = ref(false)
const giftEnabled = ref(true)
const welcomeEnabled = ref(false)
const welcomeTtsGuard = ref(false)
const fontSize = ref(17)
let unlistenOverlayVisible: UnlistenFn | undefined
let unlistenOverlayClickthrough: UnlistenFn | undefined
let unlistenOverlayAlwaysOnTop: UnlistenFn | undefined
let unlistenTtsConfig: UnlistenFn | undefined
let unlistenGiftTtsConfig: UnlistenFn | undefined
let unlistenGiftConfig: UnlistenFn | undefined
let unlistenWelcomeConfig: UnlistenFn | undefined
let unlistenFontSize: UnlistenFn | undefined

async function loadInitialState() {
  // 用 get_dashboard_status 拉初始状态，所有开关的初始值都在这里统一获取
  try {
    const status = await invoke('get_dashboard_status')
    overlayVisible.value = status.overlay.visible
    overlayClickthrough.value = status.overlay.clickthrough
    overlayAlwaysOnTop.value = status.overlay.always_on_top
  } catch {}
  try {
    const cfg = await invoke<TtsConfig>('tts_get_config')
    ttsEnabled.value = cfg.enabled
  } catch {}
  try {
    const cfg = await invoke<GiftTtsConfig>('gift_tts_get_config')
    giftTtsEnabled.value = cfg.enabled
  } catch {}
  try {
    const cfg = await invoke<GiftConfig>('gift_get_config')
    giftEnabled.value = cfg.enabled
  } catch {}
  try {
    const cfg = await invoke<WelcomeConfig>('welcome_get_config')
    welcomeEnabled.value = cfg.enabled
    welcomeTtsGuard.value = cfg.tts_guard
  } catch {}
  try {
    const style = await invoke<OverlayStyle>('overlay_get_style')
    fontSize.value = style.font_size
  } catch {}
}

onMounted(async () => {
  await loadInitialState()
  unlistenOverlayVisible = await listen<boolean>('overlay-visible', v => {
    overlayVisible.value = v
  })
  unlistenOverlayClickthrough = await listen<boolean>('overlay-clickthrough', v => {
    overlayClickthrough.value = v
  })
  unlistenOverlayAlwaysOnTop = await listen<boolean>('overlay-always-on-top', v => {
    overlayAlwaysOnTop.value = v
  })
  unlistenTtsConfig = await listen<TtsConfig>('tts-config', cfg => {
    ttsEnabled.value = cfg.enabled
  })
  unlistenGiftTtsConfig = await listen<GiftTtsConfig>('gift-tts-config', cfg => {
    giftTtsEnabled.value = cfg.enabled
  })
  unlistenGiftConfig = await listen<GiftConfig>('gift-config', cfg => {
    giftEnabled.value = cfg.enabled
  })
  unlistenWelcomeConfig = await listen<WelcomeConfig>('welcome-config', cfg => {
    welcomeEnabled.value = cfg.enabled
    welcomeTtsGuard.value = cfg.tts_guard
  })
  // 字号变化由 overlay_set_style 广播，监听 style 即可提取
  unlistenFontSize = await listen<OverlayStyle>('overlay-style', style => {
    fontSize.value = style.font_size
  })
})

onUnmounted(() => {
  unlistenOverlayVisible?.()
  unlistenOverlayClickthrough?.()
  unlistenOverlayAlwaysOnTop?.()
  unlistenTtsConfig?.()
  unlistenGiftTtsConfig?.()
  unlistenGiftConfig?.()
  unlistenWelcomeConfig?.()
  unlistenFontSize?.()
})

// 快捷控制修改函数
async function setOverlayVisible(v: boolean) {
  await invoke('overlay_set_visible', { visible: v })
}
async function setOverlayClickthrough(v: boolean) {
  await invoke('overlay_set_clickthrough', { enabled: v })
}
async function setOverlayAlwaysOnTop(v: boolean) {
  await invoke('overlay_set_always_on_top', { enabled: v })
}
async function setTtsEnabled(v: boolean) {
  await invoke('tts_set_config', { tts: { ...(await invoke<TtsConfig>('tts_get_config')), enabled: v } })
}
async function setGiftTtsEnabled(v: boolean) {
  await invoke('gift_tts_set_config', {
    giftTts: { ...(await invoke<GiftTtsConfig>('gift_tts_get_config')), enabled: v },
  })
}
async function setGiftEnabled(v: boolean) {
  await invoke('gift_set_config', { gift: { ...(await invoke<GiftConfig>('gift_get_config')), enabled: v } })
}
async function setWelcomeEnabled(v: boolean) {
  await invoke('welcome_set_config', {
    welcome: { ...(await invoke<WelcomeConfig>('welcome_get_config')), enabled: v },
  })
}
async function setWelcomeTtsGuard(v: boolean) {
  await invoke('welcome_set_config', {
    welcome: { ...(await invoke<WelcomeConfig>('welcome_get_config')), tts_guard: v },
  })
}
async function setFontSize(v: number) {
  const style = { ...(await invoke<OverlayStyle>('overlay_get_style')), font_size: v }
  await invoke('overlay_set_style', { style })
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

    <div v-if="recentRooms.length" class="recent-bar">
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

    <!-- 快捷控制卡片 -->
    <QuickControl
      :overlay-visible="overlayVisible"
      :overlay-clickthrough="overlayClickthrough"
      :overlay-always-on-top="overlayAlwaysOnTop"
      :tts-enabled="ttsEnabled"
      :gift-tts-enabled="giftTtsEnabled"
      :gift-enabled="giftEnabled"
      :welcome-enabled="welcomeEnabled"
      :welcome-tts-guard="welcomeTtsGuard"
      :font-size="fontSize"
      @update:overlay-visible="setOverlayVisible"
      @update:overlay-clickthrough="setOverlayClickthrough"
      @update:overlay-always-on-top="setOverlayAlwaysOnTop"
      @update:tts-enabled="setTtsEnabled"
      @update:gift-tts-enabled="setGiftTtsEnabled"
      @update:gift-enabled="setGiftEnabled"
      @update:welcome-enabled="setWelcomeEnabled"
      @update:welcome-tts-guard="setWelcomeTtsGuard"
      @update:font-size="setFontSize" />
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
