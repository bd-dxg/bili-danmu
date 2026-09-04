<script setup lang="ts">
import { onMounted, ref } from "vue";
import RoomView from "./views/RoomView.vue";
import LoginDialog from "./components/LoginDialog.vue";
import { refreshLogin } from "./composables/useLogin";

onMounted(refreshLogin);

type NavKey = "room" | "danmaku" | "tts" | "overlay" | "about";

const navs: { key: NavKey; label: string }[] = [
  { key: "room", label: "直播间" },
  { key: "danmaku", label: "弹幕" },
  { key: "tts", label: "朗读" },
  { key: "overlay", label: "Overlay" },
  { key: "about", label: "关于" },
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
      <RoomView v-if="current === 'room'" />      <section v-else class="placeholder">
        <h2>{{ navs.find((n) => n.key === current)?.label }}</h2>
        <p>该设置页在后续 Milestone 中实现。</p>
      </section>
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
  font-size: 15px;
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
  font-size: 13px;
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

.placeholder h2 {
  font-size: 16px;
  margin-bottom: 8px;
}

.placeholder p {
  color: var(--text-faint);
  font-size: 13px;
}
</style>
