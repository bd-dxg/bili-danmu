<script setup lang="ts">
import MetaBadges from './MetaBadges.vue'
import { rowShadow } from './row-style'

import type { DisplayBacking, OverlayStyle } from '../types/ipc'

// 单条礼物行：徽章区（与弹幕行共用）+ 正文区（用户名 / 送出 / 礼物名 / 数量 / 金额）。
// 字体与描边跟随弹幕样式，礼物名与金额用金色与弹幕正文区分。
defineProps<{
  b: DisplayBacking
  overlayStyle: OverlayStyle
}>()

/** 金额展示：分 → ¥X（整数元省略小数） */
function money(fen: number): string {
  const yuan = fen / 100
  return Number.isInteger(yuan) ? `¥${yuan}` : `¥${yuan.toFixed(2)}`
}
</script>

<template>
  <div class="gift-row" :style="{ textShadow: rowShadow(overlayStyle) }" data-tauri-drag-region>
    <MetaBadges
      :is-admin="false"
      :guard-level="b.guard_level"
      :medal-name="b.medal_name"
      :medal-level="b.medal_level"
      :is-room-medal="b.isRoomMedal"
      :wealth-level="b.wealth_level"
      :overlay-style="overlayStyle" />
    <span class="msg">
      <span
        class="user"
        :style="{
          color: overlayStyle.username_color,
          fontWeight: overlayStyle.bold ? 800 : 600,
        }">
        {{ b.username }}
      </span>
      <span class="action">{{ b.kind === 'super_chat' ? '发送' : '送出' }}</span>
      <span class="gift" :style="{ fontWeight: overlayStyle.bold ? 800 : 600 }">
        {{ b.gift_name }}
        <template v-if="b.num > 1">{{ ' ×' + b.num }}</template>
      </span>
      <!-- 醒目留言正文：超宽在正文区折行，续行与首行同列 -->
      <span v-if="b.message" class="message" :style="{ color: overlayStyle.content_color }">{{ b.message }}</span>
    </span>
    <!-- 金额独立成列、右对齐到行尾：不受用户名 / 礼物名 / 数量长度影响，永远在同一竖线上 -->
    <span class="amount" :style="{ fontWeight: overlayStyle.bold ? 800 : 600 }">
      {{ money(b.amount_fen) }}
    </span>
  </div>
</template>

<style scoped>
/* 礼物行：徽章区 + 正文区两栏，结构与弹幕行一致（左边缘对齐） */
.gift-row {
  display: flex;
  align-items: flex-start;
  font-size: inherit;
  line-height: 1.65;
  color: #fff;
}

.msg {
  flex: 1 1 auto;
  min-width: 0;
  word-break: break-all;
}

.user {
  font-weight: 600;
}

/* 动词：淡白，不与弹幕正文抢视线 */
.action {
  color: rgba(255, 255, 255, 0.72);
}

/* 礼物名：金色（与弹幕正文的白色拉开区分） */
.gift {
  color: #ffd75e;
  margin: 0 0.3em;
}

/* 金额：独立列，右对齐到行尾形成固定竖线。
   flex-shrink:0 保证不被长正文挤短，正文区（.msg）吃掉剩余宽度。 */
.amount {
  flex-shrink: 0;
  margin: 0 0.6em;
  text-align: right;
  color: #ffb43a;
  font-variant-numeric: tabular-nums;
}

/* 醒目留言正文：与弹幕正文同色，前面留一个分隔间距 */
.message {
  margin-left: 0.4em;
}
</style>
