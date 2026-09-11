<script setup lang="ts">
import { onMounted, ref } from "vue";
import RoomView from "./views/RoomView.vue";
import DanmakuView from "./views/DanmakuView.vue";
import AboutView from "./views/AboutView.vue";
import TtsView from "./views/TtsView.vue";
import StreamerView from "./views/StreamerView.vue";
import LoginDialog from "./components/LoginDialog.vue";
import { refreshLogin } from "./composables/useLogin";

onMounted(refreshLogin);

type NavKey = "room" | "danmaku" | "tts" | "streamer" | "about";

const navs: { key: NavKey; label: string }[] = [
  { key: "room", label: "直播间连接" },
  { key: "danmaku", label: "弹幕设置" },
  { key: "tts", label: "朗读设置" },
  { key: "streamer", label: "主播分区" },
  { key: "about", label: "关于软件" },
];

const current = ref<NavKey>("room");
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
          @click="current = n.key"
        >
          {{ n.label }}
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

.content {
  flex: 1;
  overflow-y: auto;
  padding: 20px;
}
</style>
