// 屏蔽 WebView2 的默认右键菜单（返回 / 刷新 / 另存为 / 打印）
//
// 不能只挂在组件根节点上：按住右键拖动、指针离开窗口后再松开时，contextmenu 事件的
// target 会落到 <html>（实测），根节点上的 @contextmenu.prevent 收不到，WebView2 就会
// 弹出宿主默认菜单，游戏里很抢鼠标（issue #37）。挂到 document 上才能覆盖所有 target。
export function disableDefaultContextMenu(): void {
  document.addEventListener('contextmenu', event => event.preventDefault())
}
