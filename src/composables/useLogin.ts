import { invoke } from '@tauri-apps/api/core'
// 登录状态（模块级单例，多组件共享）
import { ref } from 'vue'

const loggedIn = ref(false)
const uid = ref(0)
const uname = ref('')
const loginDialogOpen = ref(false)

/** 从 Rust 查询最新登录态 */
export async function refreshLogin() {
  const info = await invoke<{
    loggedIn: boolean
    uid: number
    uname?: string | null
  }>('get_login_info')
  loggedIn.value = info.loggedIn
  uid.value = info.uid
  uname.value = info.uname ?? ''
  return info
}

export function useLogin() {
  return {
    loggedIn,
    uid,
    uname,
    loginDialogOpen,
    openLoginDialog: () => {
      loginDialogOpen.value = true
    },
    closeLoginDialog: () => {
      loginDialogOpen.value = false
    },
    async logout() {
      await invoke('logout')
      loggedIn.value = false
      uid.value = 0
      uname.value = ''
    },
  }
}
