// 登录状态（模块级单例，多组件共享）
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

const loggedIn = ref(false);
const uid = ref(0);
const loginDialogOpen = ref(false);

/** 从 Rust 查询最新登录态 */
export async function refreshLogin() {
  const info = await invoke<{ loggedIn: boolean; uid: number }>("get_login_info");
  loggedIn.value = info.loggedIn;
  uid.value = info.uid;
  return info;
}

export function useLogin() {
  return {
    loggedIn,
    uid,
    loginDialogOpen,
    openLoginDialog: () => {
      loginDialogOpen.value = true;
    },
    closeLoginDialog: () => {
      loginDialogOpen.value = false;
    },
    async logout() {
      await invoke("logout");
      loggedIn.value = false;
      uid.value = 0;
    },
  };
}
