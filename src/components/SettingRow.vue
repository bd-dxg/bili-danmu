<script setup lang="ts">
// 设置页的一行：左侧标签 + 右侧控件（默认插槽）。
//
// 自身不写样式：.setting-row 与 .label 的规则在被引用页面的 scoped 样式里
// （子组件根节点会带上父作用域标记，故仍然生效）。
defineProps<{ label: string }>()
</script>

<template>
  <div class="setting-row">
    <span class="label">{{ label }}</span>
    <slot />
  </div>
</template>
