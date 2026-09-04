<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { OverlayStyle } from "../types/ipc";

const style = ref<OverlayStyle>({
  font_size: 17,
  font_family: "Microsoft YaHei UI",
  show_wealth: true,
  show_medal: true,
  show_role: true,
  username_color: "#FFFFFF",
  content_color: "#FFFFFF",
  bold: false,
  outline: true,
  outline_color: "#000000",
  outline_width: 2,
});
const savedTip = ref(false);
// 弹幕窗尺寸
const winSize = ref({ width: 480, height: 240 });

const FONT_CHOICES = [
  { label: "微软雅黑 UI", value: "Microsoft YaHei UI" },
  { label: "微软雅黑", value: "Microsoft YaHei" },
  { label: "黑体", value: "SimHei" },
  { label: "宋体", value: "SimSun" },
  { label: "楷体", value: "KaiTi" },
  { label: "等线", value: "DengXian" },
  { label: "Segoe UI", value: "Segoe UI" },
];

let tipTimer: ReturnType<typeof setTimeout> | undefined;

async function onSizeCommit() {
  await invoke("overlay_set_size", {
    width: winSize.value.width,
    height: winSize.value.height,
  });
}

async function apply() {
  await invoke("overlay_set_style", { style: { ...style.value } });
  savedTip.value = true;
  if (tipTimer) clearTimeout(tipTimer);
  tipTimer = setTimeout(() => (savedTip.value = false), 1200);
}

function onFontSizeChange() {
  apply();
}

onMounted(async () => {
  try {
    style.value = await invoke<OverlayStyle>("overlay_get_style");
  } catch (e) {
    console.error("读取样式失败", e);
  }
  try {
    const s = await invoke<{ width: number; height: number }>("overlay_get_size");
    winSize.value = s;
  } catch (e) {
    console.error("读取尺寸失败", e);
  }
});
</script>

<template>
  <div class="danmaku-page">
    <h2>弹幕样式</h2>

    <div class="setting-card">
      <div class="setting-row">
        <span class="label">字号</span>
        <div class="size-control">
          <input
            v-model.number="style.font_size"
            type="range"
            min="13"
            max="36"
            step="1"
            @change="onFontSizeChange"
          />
          <span class="value">{{ style.font_size }}px</span>
        </div>
      </div>

      <div class="setting-row">
        <span class="label">字体</span>
        <select v-model="style.font_family" class="select" @change="apply()">
          <option v-for="f in FONT_CHOICES" :key="f.value" :value="f.value">
            {{ f.label }}
          </option>
        </select>
      </div>

      <div class="setting-row">
        <span class="label">显示舰长/房管标记</span>
        <input
          v-model="style.show_role"
          type="checkbox"
          class="switch"
          @change="apply()"
        />
      </div>

      <div class="setting-row">
        <span class="label">显示粉丝牌</span>
        <input
          v-model="style.show_medal"
          type="checkbox"
          class="switch"
          @change="apply()"
        />
      </div>

      <div class="setting-row">
        <span class="label">显示荣耀等级</span>
        <input
          v-model="style.show_wealth"
          type="checkbox"
          class="switch"
          @change="apply()"
        />
      </div>

      <p class="tip">弹幕文字统一白色（黑色描边），任意背景下清晰。</p>
      <p v-if="savedTip" class="saved">✓ 已应用并保存</p>
    </div>

    <h2 class="sub-title">颜色与描边</h2>

    <div class="setting-card">
      <div class="setting-row">
        <span class="label">用户名颜色</span>
        <input
          v-model="style.username_color"
          type="color"
          class="color-picker"
          @change="apply()"
        />
      </div>
      <div class="setting-row">
        <span class="label">弹幕内容颜色</span>
        <input
          v-model="style.content_color"
          type="color"
          class="color-picker"
          @change="apply()"
        />
      </div>
      <div class="setting-row">
        <span class="label">文字加粗</span>
        <input
          v-model="style.bold"
          type="checkbox"
          class="switch"
          @change="apply()"
        />
      </div>
      <div class="setting-row">
        <span class="label">文字描边</span>
        <input
          v-model="style.outline"
          type="checkbox"
          class="switch"
          @change="apply()"
        />
      </div>
      <div class="setting-row">
        <span class="label">描边颜色</span>
        <input
          v-model="style.outline_color"
          type="color"
          class="color-picker"
          @change="apply()"
        />
      </div>
      <div class="setting-row">
        <span class="label">描边宽度</span>
        <div class="size-control">
          <input
            v-model.number="style.outline_width"
            type="range"
            min="0"
            max="5"
            step="1"
            @change="apply()"
          />
          <span class="value">{{ style.outline_width }}px</span>
        </div>
      </div>
      <p v-if="savedTip" class="saved">✓ 已应用并保存</p>
    </div>

    <h2 class="sub-title">弹幕窗大小</h2>

    <div class="setting-card">
      <div class="setting-row">
        <span class="label">宽度</span>
        <div class="size-control">
          <input
            v-model.number="winSize.width"
            type="range"
            min="240"
            max="1600"
            step="10"
            @change="onSizeCommit"
          />
          <span class="value">{{ winSize.width }}px</span>
        </div>
      </div>
      <div class="setting-row">
        <span class="label">高度</span>
        <div class="size-control">
          <input
            v-model.number="winSize.height"
            type="range"
            min="120"
            max="900"
            step="10"
            @change="onSizeCommit"
          />
          <span class="value">{{ winSize.height }}px</span>
        </div>
      </div>
      <p class="tip">调整弹幕窗宽高（位置和大小会自动保存，下次启动恢复）。</p>
    </div>
  </div>
</template>

<style scoped>
.danmaku-page {
  display: flex;
  flex-direction: column;
}

h2 {
  font-size: 16px;
  margin-bottom: 14px;
}

.sub-title {
  margin-top: 18px;
}

.setting-card {
  background: var(--bg-side);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 4px 14px;
}

.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 0;
  border-bottom: 1px solid var(--border);
  font-size: 14px;
}

.setting-row:last-of-type {
  border-bottom: none;
}

.size-control {
  display: flex;
  align-items: center;
  gap: 10px;
}

.size-control input[type="range"] {
  width: 160px;
  accent-color: var(--accent);
}

.value {
  min-width: 42px;
  text-align: right;
  color: var(--text-dim);
  font-size: 13px;
}

.select {
  background: var(--bg-elev);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  padding: 6px 10px;
  font-size: 13px;
  outline: none;
  min-width: 150px;
}

.select:focus {
  border-color: var(--accent);
}

.switch {
  width: 17px;
  height: 17px;
  accent-color: var(--accent);
}

.color-picker {
  width: 44px;
  height: 26px;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: 5px;
  background: none;
  cursor: pointer;
}

.color-picker::-webkit-color-swatch-wrapper {
  padding: 2px;
}

.color-picker::-webkit-color-swatch {
  border: none;
  border-radius: 3px;
}

.tip {
  font-size: 12px;
  color: var(--text-faint);
  padding: 4px 0 10px;
  line-height: 1.6;
}

.saved {
  color: var(--green);
  font-size: 12px;
  padding-bottom: 10px;
}
</style>
