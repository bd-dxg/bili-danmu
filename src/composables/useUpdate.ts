import { invoke } from '@tauri-apps/api/core'
// 更新检查结果（模块级单例：App 启动静默查一次，关于页读同一份）
import { ref } from 'vue'

import type { UpdateInfo } from '../types/ipc'

const checking = ref(false)
const info = ref<UpdateInfo | null>(null)
const error = ref('')

/**
 * 检查新版本
 *
 * @param silent 启动时的自动检查传 true：网络不通（含 GitHub 被墙）不该打扰用户；
 *               手动点按传 false，失败要能看到原因并可重试
 */
export async function checkUpdate(silent = false) {
  if (checking.value) return
  checking.value = true
  try {
    info.value = await invoke<UpdateInfo>('check_update')
    error.value = ''
  } catch (e) {
    if (!silent) {
      error.value = e instanceof Error ? e.message : String(e)
      info.value = null
    }
  } finally {
    checking.value = false
  }
}

/** 在系统默认浏览器打开新版本页面（WebView 内不能直接开外链，走 Rust 的 ShellExecuteW） */
export async function openUpdatePage() {
  const url = info.value?.url
  if (!url) return
  try {
    await invoke('open_url', { url })
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  }
}

export function useUpdate() {
  return { checking, info, error, checkUpdate, openUpdatePage }
}
