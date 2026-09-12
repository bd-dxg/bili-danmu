<script setup lang="ts">
import type { OverlayStyle } from '../types/ipc'

// 弹幕行 / 礼物行共用的身份徽章区：身份前缀（房管 / 舰长）+ 荣耀等级 + 粉丝牌。
// 单独成组件是为了让两区共用同一套列宽（role-slot 4.8em）——复制一份 CSS 后
// 只改单侧，就会与发送弹幕框的缩进错位（见 window.rs 的 sender_layout_metrics）。
const props = defineProps<{
  /** 是否房管（礼物事件拿不到该字段，恒传 false） */
  isAdmin: boolean
  /** 舰队等级：3 舰长 / 2 提督 / 1 总督 */
  guardLevel?: number
  medalName?: string
  medalLevel?: number
  /** 粉丝牌是否来自当前房间（决定本房牌绿 / 其它房牌灰的配色） */
  isRoomMedal: boolean
  /** 荣耀等级（全站财富等级） */
  wealthLevel?: number
  overlayStyle: OverlayStyle
}>()

/** 身份前缀列表（可叠加）：房管 + 舰长/提督/总督，按展示顺序 */
function rolesOf(): { label: string; cls: string }[] {
  const roles: { label: string; cls: string }[] = []
  if (props.isAdmin) {
    roles.push({ label: '房管', cls: 'admin' })
  }
  const g = props.guardLevel
  if (g && g >= 1) {
    // 舰队等级：3=舰长（蓝） 2=提督（紫） 1=总督（金红），三档配色区分
    const guard =
      g === 3
        ? { label: '舰长', cls: 'guard-captain' }
        : g === 2
          ? { label: '提督', cls: 'guard-admiral' }
          : { label: '总督', cls: 'guard-governor' }
    roles.push(guard)
  }
  return roles
}
</script>

<template>
  <!-- 徽章区：固定前缀列 + LV/粉丝牌等，单行不折行 -->
  <span class="meta">
    <!-- 身份前缀列（固定宽度，无前缀留空）→ LV/粉丝牌/文字各行垂直对齐 -->
    <span class="role-slot">
      <template v-if="overlayStyle.show_role">
        <span v-for="r in rolesOf()" :key="r.cls" class="chip role" :class="r.cls">
          {{ r.label }}
        </span>
      </template>
    </span>
    <template v-if="overlayStyle.show_wealth && wealthLevel && wealthLevel > 0">
      <span class="chip lv">LV{{ wealthLevel }}</span>
    </template>
    <template v-if="overlayStyle.show_medal && medalName && isRoomMedal">
      <span class="chip medal-room">{{ medalName }}{{ medalLevel ?? 0 }}</span>
    </template>
    <template v-else-if="overlayStyle.show_medal && medalName">
      <span class="chip medal-other">{{ medalName }}{{ medalLevel ?? 0 }}</span>
    </template>
  </span>
</template>

<style scoped>
/* 徽章区：横向单行，不折行不压缩 */
.meta {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  white-space: nowrap;
  /* 与正文首行文字视觉中轴对齐。用 em 不用 px：
     徽章盒高是 0.74em x 1.7，正文行高 1.65em，两者都随字号变，
     固定 px 时字号一大就往下错开（字号 20 时 3px 已偏差 3px）。 */
  margin-top: 0.3em;
}

/* 身份前缀列：宽度 4.8em + 右间距 0.7em = 正文列起点。
   间距不能再小：面板背景左边缘要落在这一段的中间，
   否则荣耀等级 / 粉丝牌徽章会贴着背景边（见 OverlayApp 的 --panel-inset）。 */
.role-slot {
  display: inline-flex;
  align-items: center;
  justify-content: flex-end;
  gap: 0.2em;
  width: 4.8em;
  margin-right: 0.7em;
  overflow: hidden;
}

/* 前缀列内 chip 间距由 gap 控制，去掉全局右 margin（否则尾 chip 与 LV 间出现空隙） */
.role-slot .chip {
  margin-right: 0;
}

/* 身份徽章：inline-block 内联排版，数字等宽，最小宽度保证短内容对齐 */
.chip {
  display: inline-block;
  vertical-align: middle;
  font-size: 0.74em;
  line-height: 1.7;
  font-weight: 700;
  padding: 0 0.45em;
  margin-right: 0.35em;
  border-radius: 0.28em;
  text-shadow: none;
  text-align: center;
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
  transform: translateY(-1px);
}

/* 徽章区内 chip 不压缩（多 chip 叠加时保持自身宽度） */
.meta .chip {
  flex-shrink: 0;
}

/* 荣耀等级徽章：固定宽度（LV5 与 LV14 同宽） */
.chip.lv {
  min-width: 5.4ch;
  color: #6b4a00;
  background: linear-gradient(180deg, #ffe08a, #f0b429);
}

/* 粉丝牌徽章：较短牌名与“名+两位数”同宽 */
.chip.medal-room,
.chip.medal-other {
  min-width: 7.2ch;
}

/* 本房粉丝牌：绿色（与提督紫拉开区分） */
.chip.medal-room {
  color: #fff;
  background: linear-gradient(180deg, #4ade80, #16a34a);
}

/* 其它房粉丝牌 */
.chip.medal-other {
  color: #d6d9de;
  background: rgba(80, 80, 90, 0.9);
}

/* 单字身份标记（房/舰）方形居中 */
.chip.role {
  min-width: 2.6ch;
  color: #fff;
}

/* 舰长：蓝色（比房管蓝略深，避免与「房」混淆） */
.chip.role.guard-captain {
  background: linear-gradient(180deg, #4facfe, #1f6fd0);
}

/* 提督：紫色（比本房粉丝牌紫更艳） */
.chip.role.guard-admiral {
  background: linear-gradient(180deg, #c084fc, #7e22ce);
}

/* 总督：金渐变红（最高档） */
.chip.role.guard-governor {
  background: linear-gradient(180deg, #ffcc4d, #e4413a);
}

/* 房管：青绿色 teal（与舰长蓝拉开区分） */
.chip.role.admin {
  background: linear-gradient(180deg, #2dd4bf, #0d9488);
}
</style>
