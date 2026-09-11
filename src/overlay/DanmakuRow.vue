<script setup lang="ts">
import MetaBadges from "./MetaBadges.vue";
import { rowShadow } from "./row-style";
import type { DisplayDanmaku, OverlayStyle } from "../types/ipc";

// 单条弹幕行：徽章区（MetaBadges，与礼物行共用）+ 正文区。
// 行内样式全部来自 OverlayStyle（由 OverlayApp 统一下发），字号/行距等继承
// 弹幕窗根节点，故这里只处理描边与文字颜色。描边计算与礼物行共用 row-style.ts。
defineProps<{
  d: DisplayDanmaku;
  overlayStyle: OverlayStyle;
}>();
</script>

<template>
  <div
    class="danmu-row"
    :style="{ textShadow: rowShadow(overlayStyle) }"
    data-tauri-drag-region
  >
    <MetaBadges
      :is-admin="d.is_admin"
      :guard-level="d.guard_level"
      :medal-name="d.medal_name"
      :medal-level="d.medal_level"
      :is-room-medal="d.isRoomMedal"
      :wealth-level="d.wealth_level"
      :overlay-style="overlayStyle"
    />
    <!-- 正文区：用户名 + 内容，超宽在此区内折行（续行与首行正文同列） -->
    <span class="msg">
      <template v-if="d.username">
        <span
          class="user"
          :style="{
            color: overlayStyle.username_color,
            fontWeight: overlayStyle.bold ? 800 : 600,
          }"
          >{{ d.username }}：</span
        >
      </template>
      <span
        class="content"
        :style="{
          color: overlayStyle.content_color,
          fontWeight: overlayStyle.bold ? 700 : 400,
        }"
        >{{ d.content }}</span
      >
    </span>
  </div>
</template>

<style scoped>
/* 弹幕行：徽章区(meta) + 正文区(msg) 两栏；正文超宽在 msg 内折行，续行与首行同列 */
.danmu-row {
  display: flex;
  align-items: flex-start;
  font-size: inherit;
  line-height: 1.65;
  color: #fff;
}

/* 正文区：占剩余宽度，长文本在此折行 */
.msg {
  flex: 1 1 auto;
  min-width: 0;
  word-break: break-all;
}

.user {
  font-weight: 600;
}

.content {
  /* 颜色可能被 JS 覆盖，继承行高 */
  font-weight: 400;
}
</style>
