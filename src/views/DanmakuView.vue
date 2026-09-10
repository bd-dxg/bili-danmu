<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import DanmakuFilterPanel from "../components/DanmakuFilterPanel.vue";
import OverlayWindowPanel from "../components/OverlayWindowPanel.vue";
import SettingRow from "../components/SettingRow.vue";
import { useSaveTip } from "../composables/useSaveTip";
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

// 页内标签：style / color / window / filter
const tab = ref<"style" | "color" | "window" | "filter">("style");

// 弹幕过滤配置（应用后 Rust 广播给 Overlay 实时生效并持久化）
const filter = ref<DanmakuFilter>({ ...DEFAULT_DANMAKU_FILTER });

const { savedTip, opError, showSaved, showError } = useSaveTip();

const FONT_CHOICES = [
  { label: "微软雅黑 UI", value: "Microsoft YaHei UI" },
  { label: "微软雅黑", value: "Microsoft YaHei" },
  { label: "黑体", value: "SimHei" },
  { label: "宋体", value: "SimSun" },
  { label: "楷体", value: "KaiTi" },
  { label: "等线", value: "DengXian" },
  { label: "Segoe UI", value: "Segoe UI" },
];

async function apply() {
  try {
    await invoke("overlay_set_style", { style: { ...style.value } });
    showSaved();
  } catch (e) {
    showError(e);
  }
}

async function applyFilter(next: DanmakuFilter) {
  filter.value = next;
  try {
    await invoke("danmaku_set_filter", { filter: { ...next } });
    showSaved();
  } catch (e) {
    showError(e);
  }
}

onMounted(async () => {
  try {
    style.value = await invoke<OverlayStyle>("overlay_get_style");
  } catch (e) {
    console.error("读取样式失败", e);
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
        <SettingRow label="字号">
          <div class="size-control">
            <input
              v-model.number="style.font_size"
              type="range"
              min="13"
              max="36"
              step="1"
              @change="apply()"
            />
            <span class="value">{{ style.font_size }}px</span>
          </div>
        </SettingRow>

        <SettingRow label="行间距">
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
        </SettingRow>

        <SettingRow label="字体">
          <select v-model="style.font_family" class="select" @change="apply()">
            <option v-for="f in FONT_CHOICES" :key="f.value" :value="f.value">
              {{ f.label }}
            </option>
          </select>
        </SettingRow>

        <SettingRow label="显示舰长/房管标记">
          <input
            v-model="style.show_role"
            type="checkbox"
            class="switch"
            @change="apply()"
          />
        </SettingRow>

        <SettingRow label="显示粉丝牌">
          <input
            v-model="style.show_medal"
            type="checkbox"
            class="switch"
            @change="apply()"
          />
        </SettingRow>

        <SettingRow label="显示荣耀等级">
          <input
            v-model="style.show_wealth"
            type="checkbox"
            class="switch"
            @change="apply()"
          />
        </SettingRow>

        <p class="tip">弹幕文字统一白色（黑色描边），任意背景下清晰。</p>
        <p v-if="savedTip" class="saved">✓ 已应用并保存</p>
      </div>
    </section>

    <section v-show="tab === 'color'" class="tab-pane">
      <div class="setting-card">
        <SettingRow label="用户名颜色">
          <input
            v-model="style.username_color"
            type="color"
            class="color-picker"
            @change="apply()"
          />
        </SettingRow>

        <SettingRow label="弹幕内容颜色">
          <input
            v-model="style.content_color"
            type="color"
            class="color-picker"
            @change="apply()"
          />
        </SettingRow>

        <SettingRow label="文字加粗">
          <input v-model="style.bold" type="checkbox" class="switch" @change="apply()" />
        </SettingRow>

        <SettingRow label="文字描边">
          <input
            v-model="style.outline"
            type="checkbox"
            class="switch"
            @change="apply()"
          />
        </SettingRow>

        <SettingRow label="描边颜色">
          <input
            v-model="style.outline_color"
            type="color"
            class="color-picker"
            @change="apply()"
          />
        </SettingRow>

        <SettingRow label="描边宽度">
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
        </SettingRow>

        <p v-if="savedTip" class="saved">✓ 已应用并保存</p>
      </div>
    </section>

    <section v-show="tab === 'window'" class="tab-pane">
      <OverlayWindowPanel />
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

/* 设置卡片 / 标签栏 / 开关 / 下拉 / 提示 / 滑块 / 数值见全局 src/styles/settings.css */
.select {
  min-width: 150px;
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
