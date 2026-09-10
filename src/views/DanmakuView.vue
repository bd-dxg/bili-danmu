<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import DanmakuFilterPanel from "../components/DanmakuFilterPanel.vue";
import {
  DEFAULT_DANMAKU_FILTER,
  type DanmakuFilter,
  type OverlayStyle,
} from "../types/ipc";

const style = ref<OverlayStyle>({
  font_size: 17,
  font_family: "Microsoft YaHei UI",
  show_wealth: true,
  show_medal: true,
  show_role: true,
  username_color: "#85DEF1",
  content_color: "#FFFFFF",
  bold: true,
  outline: true,
  outline_color: "#000000",
  outline_width: 2,
  row_gap: 0,
});
const savedTip = ref(false);
// 页内标签：style / color / window / filter
const tab = ref<"style" | "color" | "window" | "filter">("style");
// 弹幕窗尺寸
const winSize = ref({ width: 480, height: 240 });
// 弹幕窗行为开关
const ov = ref({ visible: false, clickthrough: true, alwaysOnTop: true });
const ovError = ref("");

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
// 样式/尺寸设置失败提示
const opError = ref("");

// 弹幕过滤配置（应用后 Rust 广播给 Overlay 实时生效并持久化）
const filter = ref<DanmakuFilter>({ ...DEFAULT_DANMAKU_FILTER });

async function applyFilter(next: DanmakuFilter) {
  filter.value = next;
  try {
    await invoke("danmaku_set_filter", { filter: { ...next } });
    savedTip.value = true;
    if (tipTimer) clearTimeout(tipTimer);
    tipTimer = setTimeout(() => (savedTip.value = false), 1200);
  } catch (e) {
    showOpError(e);
  }
}

function showOpError(e: unknown) {
  opError.value = String(e);
  if (tipTimer) clearTimeout(tipTimer);
  tipTimer = setTimeout(() => (opError.value = ""), 3000);
}

async function onSizeCommit() {
  try {
    await invoke("overlay_set_size", {
      width: winSize.value.width,
      height: winSize.value.height,
    });
  } catch (e) {
    showOpError(e);
  }
}

async function apply() {
  try {
    await invoke("overlay_set_style", { style: { ...style.value } });
    opError.value = "";
    savedTip.value = true;
    if (tipTimer) clearTimeout(tipTimer);
    tipTimer = setTimeout(() => (savedTip.value = false), 1200);
  } catch (e) {
    showOpError(e);
  }
}

function onFontSizeChange() {
  apply();
}

async function setOverlayVisible(v: boolean) {
  ovError.value = "";
  try {
    ov.value.visible = await invoke<boolean>("overlay_set_visible", { visible: v });
  } catch (e) {
    ovError.value = String(e);
  }
}

async function setOverlayClickthrough(v: boolean) {
  ovError.value = "";
  try {
    ov.value.clickthrough = await invoke<boolean>("overlay_set_clickthrough", {
      enabled: v,
    });
  } catch (e) {
    ovError.value = String(e);
  }
}

async function setOverlayTop(v: boolean) {
  ovError.value = "";
  try {
    ov.value.alwaysOnTop = await invoke<boolean>("overlay_set_always_on_top", {
      enabled: v,
    });
  } catch (e) {
    ovError.value = String(e);
  }
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
  // 同步弹幕窗行为开关初始态
  try {
    ov.value.visible = await invoke<boolean>("overlay_is_visible");
    ov.value.clickthrough = await invoke<boolean>("overlay_get_clickthrough");
  } catch (e) {
    console.error("读取弹幕窗状态失败", e);
  }
  // 读取过滤配置初始值
  try {
    filter.value = await invoke<DanmakuFilter>("danmaku_get_filter");
  } catch (e) {
    console.error("读取过滤配置失败", e);
  }
});
</script>

<template>
  <div class="danmaku-page">
    <p v-if="opError" class="error op-error">{{ opError }}</p>
    <div class="tabs">
      <button
        v-for="t in [
          { key: 'style', label: '弹幕样式' },
          { key: 'color', label: '颜色与描边' },
          { key: 'window', label: '弹幕窗' },
          { key: 'filter', label: '弹幕过滤' },
        ]"
        :key="t.key"
        class="tab-btn"
        :class="{ active: tab === t.key }"
        @click="tab = t.key as 'style' | 'color' | 'window' | 'filter'"
      >
        {{ t.label }}
      </button>
    </div>

    <section v-show="tab === 'style'" class="tab-pane">

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
        <span class="label">行间距</span>
        <div class="size-control">
          <input
            v-model.number="style.row_gap"
            type="range"
            min="0"
            max="30"
            step="1"
            @change="apply()"
          />
          <span class="value">{{ style.row_gap }}px</span>
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

    </section>

    <section v-show="tab === 'color'" class="tab-pane">

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

    </section>

    <section v-show="tab === 'window'" class="tab-pane">

    <div class="setting-card">
      <p v-if="ovError" class="error">{{ ovError }}</p>

      <div class="setting-row">
        <span class="label">显示弹幕窗</span>
        <input
          type="checkbox"
          class="switch"
          :checked="ov.visible"
          @change="setOverlayVisible(($event.target as HTMLInputElement).checked)"
        />
      </div>

      <div class="setting-row">
        <span class="label">鼠标穿透</span>
        <input
          type="checkbox"
          class="switch"
          :checked="ov.clickthrough"
          @change="setOverlayClickthrough(($event.target as HTMLInputElement).checked)"
        />
      </div>

      <div class="setting-row">
        <span class="label">始终置顶</span>
        <input
          type="checkbox"
          class="switch"
          :checked="ov.alwaysOnTop"
          @change="setOverlayTop(($event.target as HTMLInputElement).checked)"
        />
      </div>

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
      <p class="tip">
        关闭穿透后，可在弹幕窗上按住拖动、边缘调整大小；开启穿透后鼠标点击会落到弹幕窗下方的窗口。
        位置和大小自动保存，下次启动恢复。
      </p>
    </div>

    </section>

    <section v-show="tab === 'filter'" class="tab-pane">

    <DanmakuFilterPanel :model-value="filter" verb="显示" @change="applyFilter" />
    <p v-if="savedTip" class="saved">✓ 已应用并保存</p>

    </section>
  </div>
</template>

<style scoped>
.danmaku-page {
  display: flex;
  flex-direction: column;
}

.op-error {
  margin-bottom: 10px;
}

/* 页内标签栏 */
.tabs {
  display: flex;
  gap: 2px;
  border-bottom: 1px solid var(--border);
  margin-bottom: 14px;
}

.tab-btn {
  background: none;
  border: none;
  color: var(--text-dim);
  font-size: 17px;
  font-weight: bold;
  padding: 8px 16px 10px;
  border-bottom: 2px solid transparent;
  margin-bottom: -1px;
  cursor: pointer;
}

.tab-btn:hover {
  color: var(--text);
}

.tab-btn.active {
  color: var(--accent);
  border-bottom-color: var(--accent);
  font-weight: 600;
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
  font-size: 16px;
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
  font-size: 15px;
}

.select {
  background: var(--bg-elev);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  padding: 6px 10px;
  font-size: 15px;
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
  font-size: 14px;
  color: var(--text-faint);
  padding: 4px 0 10px;
  line-height: 1.6;
}

.error {
  color: var(--red);
  font-size: 15px;
  padding: 6px 0 0;
}

.saved {
  color: var(--green);
  font-size: 14px;
  padding-bottom: 10px;
}
</style>
