<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { computed, onMounted, onUnmounted, ref } from 'vue'

import type { OverlayStyle, RoomStatusEvent } from '../types/ipc'

const danmakuText = ref('')
const sending = ref(false)
const sendError = ref('')
const loggedIn = ref(false)
const connected = ref(false)
// 弹幕字号（与弹幕窗统一，界面元素按 em 相对缩放）
const fontSize = ref(17)

let unlistenRoom: UnlistenFn | undefined
let unlistenStyle: UnlistenFn | undefined
let pollTimer: ReturnType<typeof setInterval> | undefined

const canSend = computed(() => loggedIn.value && connected.value)

async function sendDanmaku() {
  const text = danmakuText.value.trim()
  if (!text || sending.value || !canSend.value) return
  sendError.value = ''
  sending.value = true
  try {
    await invoke('send_danmaku', { msg: text })
    danmakuText.value = ''
  } catch (e) {
    sendError.value = String(e)
  } finally {
    sending.value = false
  }
}

async function poll() {
  try {
    const info = await invoke<{ loggedIn: boolean }>('get_login_info')
    loggedIn.value = info.loggedIn
  } catch {
    /* ignore */
  }
  try {
    const st = await invoke<RoomStatusEvent>('get_connection_status')
    connected.value = st.state === 'connected'
  } catch {
    /* ignore */
  }
}

onMounted(async () => {
  unlistenRoom = await listen<RoomStatusEvent>('room-status', e => {
    connected.value = e.payload.state === 'connected'
  })
  unlistenStyle = await listen<OverlayStyle>('overlay-style', e => {
    fontSize.value = e.payload.font_size
  })
  try {
    const s = await invoke<OverlayStyle>('overlay_get_style')
    fontSize.value = s.font_size
  } catch {
    /* ignore */
  }
  await poll()
  pollTimer = setInterval(poll, 2000)
})

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer)
  unlistenRoom?.()
  unlistenStyle?.()
})
</script>

<template>
  <div class="sender-root" :style="{ fontSize: fontSize + 'px' }">
    <input
      v-model="danmakuText"
      class="send-input"
      type="text"
      placeholder="输入弹幕，回车发送"
      maxlength="100"
      :disabled="!canSend"
      @keyup.enter="sendDanmaku" />
    <div v-if="sendError" class="status-line">
      <span class="err">{{ sendError }}</span>
    </div>
  </div>
</template>

<style scoped>
.sender-root {
  display: flex;
  flex-direction: column;
  height: 100%;
  box-sizing: border-box;
  overflow: hidden;
  padding: 0.25em 0;
}

.send-input {
  width: 100%;
  appearance: none;
  -webkit-appearance: none;
  background: rgba(0, 0, 0, 0.55);
  border: 1px solid rgba(255, 255, 255, 0.35);
  border-radius: 0.4em;
  color: #fff;
  padding: 0.4em 0.6em;
  font-size: 1em;
  outline: none;
  user-select: text;
}

.send-input::placeholder {
  color: rgba(255, 255, 255, 0.5);
}

.send-input:focus {
  border-color: rgba(120, 180, 255, 0.95);
  border-radius: 0.4em;
}

.status-line {
  padding: 0.15em 0.15em 0;
  font-size: 0.75em;
  flex-shrink: 0;
  min-height: 1.2em;
  color: rgba(255, 255, 255, 0.85);
  text-shadow:
    0 0 2px rgba(0, 0, 0, 0.9),
    0 0 2px rgba(0, 0, 0, 0.9);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.err {
  color: #ff7b72;
}
</style>
