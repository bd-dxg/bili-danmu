<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import SettingRow from "./SettingRow.vue";

// 「弹幕窗」标签页：显示 / 鼠标穿透 / 始终置顶 / 宽高。
//
// 状态全部留在本组件内（与弹幕样式、弹幕过滤无耦合），父页面只管挂载。
// 设置卡片、设置行与开关的样式来自全局 styles/settings.css；
// 失败提示用独立的 .panel-error，避免与页面的 .error 样式互相影响。
const winSize = ref({ width: 480, height: 240 });
// 始终置顶没有 getter，初值沿用创建窗口时的 true
const ov = ref({ visible: false, clickthrough: true, alwaysOnTop: true });
const errMsg = ref("");
let errTimer: ReturnType<typeof setTimeout> | undefined;

/** 操作失败就地提示（3s 后自动消失），不打断页面其它区域的保存提示 */
function fail(e: unknown) {
  errMsg.value = String(e);
  if (errTimer) clearTimeout(errTimer);
  errTimer = setTimeout(() => (errMsg.value = ""), 3000);
}

async function setVisible(v: boolean) {
  errMsg.value = "";
  try {
    ov.value.visible = await invoke<boolean>("overlay_set_visible", { visible: v });
  } catch (e) {
    fail(e);
  }
}

async function setClickthrough(v: boolean) {
  errMsg.value = "";
  try {
    ov.value.clickthrough = await invoke<boolean>("overlay_set_clickthrough", {
      enabled: v,
    });
  } catch (e) {
    fail(e);
  }
}

async function setAlwaysOnTop(v: boolean) {
  errMsg.value = "";
  try {
    ov.value.alwaysOnTop = await invoke<boolean>("overlay_set_always_on_top", {
      enabled: v,
    });
  } catch (e) {
    fail(e);
  }
}

/** 提交窗口宽高（滑块拖动结束后才调用，避免拖动中反复改窗口） */
async function commitSize() {
  try {
    await invoke("overlay_set_size", {
      width: winSize.value.width,
      height: winSize.value.height,
    });
  } catch (e) {
    fail(e);
  }
}

onMounted(async () => {
  try {
    winSize.value = await invoke<{ width: number; height: number }>("overlay_get_size");
  } catch (e) {
    console.error("读取尺寸失败", e);
  }
  try {
    ov.value.visible = await invoke<boolean>("overlay_is_visible");
    ov.value.clickthrough = await invoke<boolean>("overlay_get_clickthrough");
  } catch (e) {
    console.error("读取弹幕窗状态失败", e);
  }
});

onUnmounted(() => {
  if (errTimer) clearTimeout(errTimer);
});
</script>

<template>
  <div class="setting-card">
    <p v-if="errMsg" class="panel-error">{{ errMsg }}</p>

    <SettingRow label="显示弹幕窗">
      <input
        type="checkbox"
        class="switch"
        :checked="ov.visible"
        @change="setVisible(($event.target as HTMLInputElement).checked)"
      />
    </SettingRow>

    <SettingRow label="鼠标穿透">
      <input
        type="checkbox"
        class="switch"
        :checked="ov.clickthrough"
        @change="setClickthrough(($event.target as HTMLInputElement).checked)"
      />
    </SettingRow>

    <SettingRow label="始终置顶">
      <input
        type="checkbox"
        class="switch"
        :checked="ov.alwaysOnTop"
        @change="setAlwaysOnTop(($event.target as HTMLInputElement).checked)"
      />
    </SettingRow>

    <SettingRow label="宽度">
      <div class="size-control">
        <input
          v-model.number="winSize.width"
          type="range"
          min="240"
          max="1600"
          step="10"
          @change="commitSize"
        />
        <span class="value">{{ winSize.width }}px</span>
      </div>
    </SettingRow>

    <SettingRow label="高度">
      <div class="size-control">
        <input
          v-model.number="winSize.height"
          type="range"
          min="120"
          max="900"
          step="10"
          @change="commitSize"
        />
        <span class="value">{{ winSize.height }}px</span>
      </div>
    </SettingRow>

    <p class="tip">
      关闭穿透后，可在弹幕窗上按住拖动、边缘调整大小；开启穿透后鼠标点击会落到弹幕窗下方的窗口。
      位置和大小自动保存，下次启动恢复。
    </p>
  </div>
</template>

<style scoped>
/* 滑块与数值标签见全局 src/styles/settings.css（.size-control / .value） */

/* 只在本标签页内提示，不与页面顶部的样式/过滤保存提示混用 */
.panel-error {
  color: var(--red);
  font-size: 15px;
  padding: 6px 0 0;
}
</style>
