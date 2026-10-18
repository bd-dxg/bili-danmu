<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'

import LoginDialog from './components/LoginDialog.vue'
import { refreshLogin } from './composables/useLogin'
import { startUpdatePolling, useUpdate } from './composables/useUpdate'
import AboutView from './views/AboutView.vue'
import DanmakuView from './views/DanmakuView.vue'
import RoomView from './views/RoomView.vue'
import StreamerView from './views/StreamerView.vue'
import TtsView from './views/TtsView.vue'

// 侧边栏红点的数据源（与关于页读的是同一个模块级单例）
const { latest: newVersion } = useUpdate()
const hasUpdate = computed(() => Boolean(newVersion.value))

onMounted(() => {
  refreshLogin()
  // 启动 5s 后查一次 + 之后每 6 小时一次：主播常连续开播几天不重启，只查一次不够；
  // 结果不弹窗，靠侧边栏「关于软件」红点与关于页徽章展示（silent 失败不打扰）
  startUpdatePolling()
})

type NavKey = 'room' | 'danmaku' | 'tts' | 'streamer' | 'about'

const navs: { key: NavKey; label: string }[] = [
  { key: 'room', label: '直播间连接' },
  { key: 'danmaku', label: '弹幕设置' },
  { key: 'tts', label: '朗读设置' },
  { key: 'streamer', label: '主播分区' },
  { key: 'about', label: '关于软件' },
]

const current = ref<NavKey>('room')
</script>

<template>
  <div class="layout">
    <aside class="sidebar">
      <div class="brand">bili-danmu</div>
      <nav>
        <button
          v-for="n in navs"
          :key="n.key"
          class="nav-item"
          :class="{ active: current === n.key }"
          @click="current = n.key">
          {{ n.label }}
          <!-- 有新版本：不占行的小红点（不是「未读」，是「可更新」，点进去看过也不会消失） -->
          <span v-if="n.key === 'about' && hasUpdate" class="nav-dot" />
        </button>
      </nav>
    </aside>
    <main class="content">
      <!-- v-show 常驻渲染：切换页面不销毁组件，连接状态/弹幕列表保留 -->
      <RoomView v-show="current === 'room'" />
      <DanmakuView v-show="current === 'danmaku'" />
      <TtsView v-show="current === 'tts'" />
      <StreamerView v-show="current === 'streamer'" />
      <AboutView v-show="current === 'about'" />
    </main>
  </div>
  <LoginDialog />
</template>

<style scoped>
.layout {
  display: flex;
  height: 100%;
}

.sidebar {
  width: 150px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  background: var(--bg-side);
}

.brand {
  padding: 16px 14px;
  font-weight: 600;
  font-size: 17px;
  color: var(--accent);
  border-bottom: 1px solid var(--border);
}

nav {
  display: flex;
  flex-direction: column;
  padding: 8px;
  gap: 2px;
}

.nav-item {
  /* 红点的定位基准 */
  position: relative;
  background: none;
  border: none;
  color: var(--text-dim);
  text-align: left;
  padding: 9px 12px;
  border-radius: 6px;
  font-size: 15px;
  font-weight: 600;
}

.nav-item:hover {
  background: var(--hover);
}

.nav-item.active {
  background: var(--accent-soft);
  color: var(--accent-strong-text);
}

/* 有新版本的提示点：绝对定位，不撑宽也不推挤文字（侧边栏只有 150px 宽） */
.nav-dot {
  position: absolute;
  top: 50%;
  right: 10px;
  width: 7px;
  height: 7px;
  margin-top: -3.5px;
  border-radius: 50%;
  background: var(--red);
}

.content {
  flex: 1;
  overflow-y: auto;
  padding: 16px;
}
</style>
