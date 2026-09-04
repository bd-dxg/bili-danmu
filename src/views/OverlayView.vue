<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

const visible = ref(false);
const clickthrough = ref(true);
const alwaysOnTop = ref(true);
const boundary = ref(false);
const loading = ref(true);
const errorMsg = ref("");

type ToggleResult = { ok: boolean; value?: boolean; message?: string };

async function wrap(fn: () => Promise<unknown>): Promise<boolean | null> {
  try {
    return (await fn()) as boolean | null;
  } catch (e) {
    errorMsg.value = String(e);
    return null;
  }
}

async function toggleVisible() {
  const v = await wrap(() => invoke("overlay_set_visible", { visible: !visible.value }));
  if (v !== null) visible.value = v;
}

async function toggleClickthrough() {
  const v = await wrap(() =>
    invoke("overlay_set_clickthrough", { enabled: !clickthrough.value }),
  );
  if (v !== null) clickthrough.value = v;
}

async function toggleTop() {
  const v = await wrap(() =>
    invoke("overlay_set_always_on_top", { enabled: !alwaysOnTop.value }),
  );
  if (v !== null) alwaysOnTop.value = v;
}

async function toggleBoundary() {
  const v = await wrap(() =>
    invoke("overlay_set_boundary", { show: !boundary.value }),
  );
  if (v !== null) boundary.value = v;
}

onMounted(async () => {
  try {
    visible.value = await invoke<boolean>("overlay_is_visible");
    clickthrough.value = await invoke<boolean>("overlay_get_clickthrough");
    boundary.value = await invoke<boolean>("overlay_get_boundary");
  } catch (e) {
    errorMsg.value = String(e);
  } finally {
    loading.value = false;
  }
});
</script>

<template>
  <div class="overlay-page">
    <h2>Overlay 悬浮窗</h2>

    <p v-if="errorMsg" class="error">{{ errorMsg }}</p>

    <div class="toggle-row">
      <span class="label">显示 Overlay</span>
      <button class="btn" :class="visible ? 'on' : 'off'" @click="toggleVisible">
        {{ visible ? "开" : "关" }}
      </button>
    </div>

    <div class="toggle-row">
      <span class="label">鼠标穿透</span>
      <button
        class="btn"
        :class="clickthrough ? 'on' : 'off'"
        @click="toggleClickthrough"
      >
        {{ clickthrough ? "开" : "关" }}
      </button>
    </div>

    <div class="toggle-row">
      <span class="label">显示边界参考线</span>
      <button class="btn" :class="boundary ? 'on' : 'off'" @click="toggleBoundary">
        {{ boundary ? "开" : "关" }}
      </button>
    </div>

    <div class="toggle-row">
      <span class="label">始终置顶</span>
      <button
        class="btn"
        :class="alwaysOnTop ? 'on' : 'off'"
        @click="toggleTop"
      >
        {{ alwaysOnTop ? "开" : "关" }}
      </button>
    </div>

    <div class="tips">
      <p>1. 连接直播间后，Overlay 自动显示收到的弹幕</p>
      <p>2. "显示边界参考线"开启后弹幕窗四周有黑白圈，方便拖拽/调整大小</p>
      <p>3. 关闭鼠标穿透后，可在 Overlay 上按住拖动窗口，边缘可调整大小</p>
      <p>4. 开启穿透后，鼠标点击会直接落到 Overlay 下方的窗口</p>
    </div>
  </div>
</template>

<style scoped>
.overlay-page {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

h2 {
  font-size: 16px;
  margin-bottom: 6px;
}

.error {
  color: var(--red);
  font-size: 13px;
}

.toggle-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--bg-side);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 8px 12px;
}

.label {
  font-size: 14px;
}

.btn {
  border: none;
  border-radius: 5px;
  padding: 4px 16px;
  font-size: 13px;
  cursor: pointer;
  min-width: 52px;
}

.btn.on {
  background: var(--accent);
  color: #fff;
}

.btn.off {
  background: var(--bg-elev);
  color: var(--text-dim);
  border: 1px solid var(--border);
}

.btn.on:hover {
  background: var(--accent-hover);
}

.btn.off:hover {
  background: var(--hover);
}

.tips {
  margin-top: 8px;
  border-top: 1px solid var(--border);
  padding-top: 12px;
  font-size: 12px;
  color: var(--text-faint);
  line-height: 1.9;
}
</style>
