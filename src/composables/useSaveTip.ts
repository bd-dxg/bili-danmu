import { onUnmounted, ref } from 'vue'

/**
 * 设置页的轻提示：同一时刻只显示「已保存」或「操作失败」中的一条，到时自动消失。
 *
 * 两个状态互斥（显示其一会清掉另一个），避免保存成功后错误提示还挂在那里。
 */
export function useSaveTip() {
  const savedTip = ref(false)
  const opError = ref('')
  let timer: ReturnType<typeof setTimeout> | undefined

  function clearTimer() {
    if (timer) clearTimeout(timer)
    timer = undefined
  }

  /** 显示「已应用并保存」（1.2s 后自动消失） */
  function showSaved() {
    clearTimer()
    opError.value = ''
    savedTip.value = true
    timer = setTimeout(() => (savedTip.value = false), 1200)
  }

  /** 显示操作失败原因（3s 后自动消失） */
  function showError(e: unknown) {
    clearTimer()
    savedTip.value = false
    opError.value = String(e)
    timer = setTimeout(() => (opError.value = ''), 3000)
  }

  onUnmounted(clearTimer)

  return { savedTip, opError, showSaved, showError }
}
