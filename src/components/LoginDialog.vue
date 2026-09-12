<script setup lang="ts">
import { invoke } from '@tauri-apps/api/core'
import QRCode from 'qrcode'
import { onMounted, onUnmounted, ref } from 'vue'

import { refreshLogin, useLogin } from '../composables/useLogin'

const { loginDialogOpen, closeLoginDialog } = useLogin()

const QRData = ref<{ url: string; key: string } | null>(null)
const qrImage = ref('')
const loading = ref(true)
const phase = ref<'scanning' | 'confirmed' | 'success' | 'expired' | 'error'>('scanning')
const message = ref('')

let pollTimer: ReturnType<typeof setInterval> | undefined

async function startLogin() {
  loading.value = true
  phase.value = 'scanning'
  message.value = ''
  try {
    const data = await invoke<{ url: string; key: string }>('qr_generate')
    QRData.value = data
    qrImage.value = await QRCode.toDataURL(data.url, { width: 220, margin: 1 })
    loading.value = false
    startPolling(data.key)
  } catch (e) {
    phase.value = 'error'
    message.value = `生成二维码失败: ${e}`
    loading.value = false
  }
}

function startPolling(key: string) {
  stopPolling()
  pollTimer = setInterval(async () => {
    try {
      const res = await invoke<{
        status: string
        message?: string
        cookies?: string
      }>('qr_poll', { key })
      // Rust serde tag 输出为 PascalCase
      switch (res.status) {
        case 'Success':
          phase.value = 'success'
          message.value = '登录成功'
          stopPolling()
          await refreshLogin()
          setTimeout(() => closeLoginDialog(), 600)
          break
        case 'Confirmed':
          phase.value = 'confirmed'
          message.value = '已扫码，请在手机上确认登录'
          break
        case 'Scanning':
          phase.value = 'scanning'
          break
        case 'Expired':
          phase.value = 'expired'
          message.value = '二维码已过期，请重新生成'
          stopPolling()
          break
        default:
          phase.value = 'error'
          message.value = res.message ?? '未知错误'
          stopPolling()
      }
    } catch (e) {
      phase.value = 'error'
      message.value = String(e)
      stopPolling()
    }
  }, 1500)
}

function stopPolling() {
  if (pollTimer) {
    clearInterval(pollTimer)
    pollTimer = undefined
  }
}

onMounted(startLogin)
onUnmounted(stopPolling)
</script>

<template>
  <div v-if="loginDialogOpen" class="dialog-mask" @click.self="closeLoginDialog">
    <div class="dialog">
      <h3>扫码登录 B 站</h3>
      <p class="hint">B 站要求登录后才能接收弹幕，登录信息仅保存在本机</p>

      <div v-if="loading" class="qr-placeholder">加载二维码中…</div>
      <img v-else-if="qrImage" :src="qrImage" class="qr" alt="登录二维码" />

      <p class="phase" :class="phase">
        <template v-if="phase === 'scanning'">请使用 B 站 App 扫码</template>
        <template v-else-if="phase === 'confirmed'">已扫码，请在手机上确认</template>
        <template v-else-if="phase === 'success'">✓ 登录成功</template>
        <template v-else>{{ message }}</template>
      </p>

      <div class="actions">
        <template v-if="phase === 'expired' || phase === 'error'">
          <button class="btn primary" @click="startLogin">重新生成二维码</button>
        </template>
        <button class="btn ghost" @click="closeLoginDialog">关闭</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dialog-mask {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 100;
}

.dialog {
  background: var(--bg-side);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 22px 26px;
  width: 300px;
  text-align: center;
}

h3 {
  font-size: 17px;
  margin-bottom: 6px;
}

.hint {
  color: var(--text-faint);
  font-size: 14px;
  margin-bottom: 14px;
}

.qr {
  width: 220px;
  height: 220px;
  background: #fff;
  border-radius: 6px;
}

.qr-placeholder {
  width: 220px;
  height: 220px;
  display: flex;
  align-items: center;
  justify-content: center;
  color: var(--text-faint);
  font-size: 15px;
  background: var(--bg-elev);
  border-radius: 6px;
}

.phase {
  margin-top: 12px;
  font-size: 15px;
  min-height: 18px;
  color: var(--text-dim);
}

.phase.success {
  color: var(--green);
}

.phase.expired,
.phase.error {
  color: var(--red);
}

.actions {
  margin-top: 10px;
  display: flex;
  justify-content: center;
  gap: 8px;
}

.btn {
  border: none;
  border-radius: 6px;
  padding: 7px 14px;
  font-size: 15px;
  cursor: pointer;
}

.btn.primary {
  background: var(--accent);
  color: #fff;
}

.btn.primary:hover {
  background: var(--accent-hover);
}

.btn.ghost {
  background: none;
  color: var(--text-dim);
  border: 1px solid var(--border);
}

.btn.ghost:hover {
  background: var(--hover);
}
</style>
