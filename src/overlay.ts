// Overlay 窗口入口（独立于主界面，背景透明）
import { createApp } from 'vue'

import { disableDefaultContextMenu } from './context-menu'
import OverlayApp from './overlay/OverlayApp.vue'
import './overlay.css'

disableDefaultContextMenu()
createApp(OverlayApp).mount('#app')
