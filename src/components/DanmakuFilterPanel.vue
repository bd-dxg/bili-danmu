<script setup lang="ts">
import { ref, watch } from 'vue'

import { DEFAULT_DANMAKU_FILTER, type DanmakuFilter } from '../types/ipc'

// 弹幕过滤表单：显示筛选（DanmakuView）与朗读筛选（TtsView）共用同一套规则，
// 仅「显示 / 朗读」措辞不同，避免两处 markup 各改一遍后语义漂移。
const props = defineProps<{
  modelValue: DanmakuFilter
  /** 规则动词，用于文案：只「显示」/ 只「朗读」 */
  verb: string
}>()

const emit = defineEmits<{ change: [DanmakuFilter] }>()

const draft = ref<DanmakuFilter>({ ...DEFAULT_DANMAKU_FILTER })
// 敏感词文本（textarea 编辑态，保存时解析为词表）
const sensitiveText = ref('')

// 父组件替换整个对象时同步回表单；不深度监听，避免保存回写把输入中的文本打断
watch(() => props.modelValue, sync, { immediate: true })

function sync(value: DanmakuFilter) {
  draft.value = {
    ...DEFAULT_DANMAKU_FILTER,
    ...value,
    sensitive_words: [...(value.sensitive_words ?? [])],
  }
  sensitiveText.value = draft.value.sensitive_words.join('\n')
}

/** 解析敏感词文本：按换行/中英文逗号/顿号分隔，去空去重 */
function parseWords(text: string): string[] {
  return [
    ...new Set(
      text
        .split(/[\n,，、]/)
        .map(s => s.trim())
        .filter(Boolean),
    ),
  ]
}

function emitChange() {
  // 数字输入框清空时 v-model.number 为 ''，避免脏值传给 Rust u32 反序列化报错
  if (!Number.isFinite(draft.value.wealth_min)) {
    draft.value.wealth_min = 0
  }
  emit('change', {
    ...draft.value,
    sensitive_words: parseWords(sensitiveText.value),
  })
}
</script>

<template>
  <div class="setting-card">
    <div class="setting-row">
      <span class="label">只{{ verb }}舰长 / 房管弹幕</span>
      <input v-model="draft.enable_guard_admin" type="checkbox" class="switch" @change="emitChange()" />
    </div>

    <div class="setting-row">
      <span class="label">只{{ verb }}有粉丝牌的弹幕</span>
      <input v-model="draft.enable_medal" type="checkbox" class="switch" @change="emitChange()" />
    </div>

    <div class="setting-row">
      <span class="label">
        只{{ verb }}荣耀等级 ≥
        <input
          v-model.number="draft.wealth_min"
          type="number"
          min="0"
          max="100"
          class="num-inline"
          @change="emitChange()" />
        的弹幕
      </span>
      <input v-model="draft.enable_wealth" type="checkbox" class="switch" @change="emitChange()" />
    </div>
    <p class="tip">
      同时打开多条时，满足任意一条就显示（比如同时开「舰长 / 房管」和「有粉丝牌」，这两种人的弹幕都会显示）。 都不打开 =
      谁的弹幕都显示。
    </p>
  </div>

  <div class="setting-card words-card">
    <div class="setting-row">
      <span class="label">屏蔽含敏感词的弹幕</span>
      <input v-model="draft.enable_sensitive" type="checkbox" class="switch" @change="emitChange()" />
    </div>
    <p class="tip">
      每行一个关键词（也可以用逗号隔开）。弹幕里出现其中任何一个词，整条就不{{ verb }}；
      上面的身份筛选拦不住它——舰长、房管的发言一样会被屏蔽。
    </p>
    <textarea
      v-model="sensitiveText"
      class="words-input"
      rows="5"
      placeholder="每行一个关键词，如：加群 / 广告, 代练"></textarea>
    <div class="words-actions">
      <button class="save-btn" @click="emitChange()">保存敏感词</button>
    </div>
  </div>
</template>

<style scoped>
/* 设置卡片 / 设置行 / 开关 / 提示见全局 src/styles/settings.css */

/* 荣耀等级阈值内联输入 */
.num-inline {
  width: 56px;
  background: var(--bg-elev);
  border: 1px solid var(--border);
  border-radius: 5px;
  color: var(--text);
  padding: 3px 6px;
  font-size: 15px;
  outline: none;
  text-align: center;
}

.num-inline:focus {
  border-color: var(--accent);
}

/* 敏感词输入区 */
.words-card {
  margin-top: 14px;
}

.words-input {
  width: 100%;
  box-sizing: border-box;
  background: var(--bg-elev);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  padding: 8px 10px;
  font-size: 15px;
  font-family: inherit;
  line-height: 1.6;
  resize: vertical;
  outline: none;
}

.words-input:focus {
  border-color: var(--accent);
}

.words-actions {
  display: flex;
  justify-content: flex-end;
  padding: 10px 0 12px;
}
</style>
