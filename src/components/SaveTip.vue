<script setup lang="ts">
// 保存 / 错误提示：固定浮层，不占文档流
//
// 原先各页把提示写成卡片内的 `<p v-if="savedTip">`，一出现就把下面的内容顶下去（页面抖动）。
// 换成右下角浮层后，出现/消失都不影响布局；三个设置页共用这一份。
// 外观在全局 `src/styles/settings.css` 的 `.save-tip`。
defineProps<{
  /** 是否显示「已应用并保存」 */
  saved: boolean
  /** 错误文案（非空优先显示） */
  error: string
}>()
</script>

<template>
  <p v-if="error" class="save-tip save-tip-error">{{ error }}</p>
  <p v-else-if="saved" class="save-tip">✓ 已应用并保存</p>
</template>
