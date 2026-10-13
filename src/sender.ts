// 发送框窗口入口（独立悬浮小窗，背景透明与弹幕窗一致）
import { createApp } from 'vue'

import { disableDefaultContextMenu } from './context-menu'
import SenderApp from './sender/SenderApp.vue'
import './sender.css'

disableDefaultContextMenu()
createApp(SenderApp).mount('#app')
