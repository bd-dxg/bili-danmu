<script setup lang="ts">
import { computed } from "vue";
import type { DisplayDanmaku, OverlayStyle } from "../types/ipc";

// 单条弹幕行：徽章区（身份前缀 / LV / 粉丝牌）+ 正文区。
// 行内样式全部来自 OverlayStyle（由 OverlayApp 统一下发），字号/行距等继承
// 弹幕窗根节点，故这里只处理描边与文字颜色。
const props = defineProps<{
  d: DisplayDanmaku;
  overlayStyle: OverlayStyle;
}>();

/** 描边 → 四向 text-shadow（硬边，宽度近似） */
const rowShadow = computed(() => {
  const s = props.overlayStyle;
  if (!s.outline) return "none";
  const w = Math.min(5, Math.max(0, Math.round(s.outline_width)));
  const shadows: string[] = [];
  for (let i = 1; i <= w; i++) {
    shadows.push(
      `0 ${i}px 0 ${s.outline_color}`,
      `0 -${i}px 0 ${s.outline_color}`,
      `${i}px 0 0 ${s.outline_color}`,
      `-${i}px 0 0 ${s.outline_color}`,
    );
  }
  return shadows.join(", ");
});

/** 身份前缀列表（可叠加）：房管 + 舰长/提督/总督，按展示顺序 */
function rolesOf(d: DisplayDanmaku): { label: string; cls: string }[] {
  const roles: { label: string; cls: string }[] = [];
  if (d.is_admin) {
    roles.push({ label: "房管", cls: "admin" });
  }
  if (d.guard_level && d.guard_level >= 1) {
    // 舰队等级：3=舰长（蓝） 2=提督（紫） 1=总督（金红），三档配色区分
    const guard =
      d.guard_level === 3
        ? { label: "舰长", cls: "guard-captain" }
        : d.guard_level === 2
          ? { label: "提督", cls: "guard-admiral" }
          : { label: "总督", cls: "guard-governor" };
    roles.push(guard);
  }
  return roles;
}
</script>

<template>
  <div
    class="danmu-row"
    :style="{ textShadow: rowShadow }"
    data-tauri-drag-region
  >
    <!-- 徽章区：固定前缀列 + LV/粉丝牌等，单行不折行 -->
    <span class="meta">
      <!-- 身份前缀列（固定宽度，无前缀留空）→ LV/粉丝牌/文字各行垂直对齐 -->
      <span class="role-slot">
        <template v-if="overlayStyle.show_role">
          <span v-for="r in rolesOf(d)" :key="r.cls" class="chip role" :class="r.cls">
            {{ r.label }}
          </span>
        </template>
      </span>
      <template v-if="overlayStyle.show_wealth && d.wealth_level && d.wealth_level > 0">
        <span class="chip lv">LV{{ d.wealth_level }}</span>
      </template>
      <template v-if="overlayStyle.show_medal && d.medal_name && d.isRoomMedal">
        <span class="chip medal-room">
          {{ d.medal_name }}{{ d.medal_level ?? 0 }}
        </span>
      </template>
      <template v-else-if="overlayStyle.show_medal && d.medal_name">
        <span class="chip medal-other">
          {{ d.medal_name }}{{ d.medal_level ?? 0 }}
        </span>
      </template>
    </span>
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
  text-shadow:
    0 0 2px rgba(0, 0, 0, 0.95),
    0 0 2px rgba(0, 0, 0, 0.95),
    1px 1px 1px rgba(0, 0, 0, 0.95),
    -1px -1px 1px rgba(0, 0, 0, 0.95);
}

/* 徽章区：横向单行，不折行不压缩 */
.meta {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  white-space: nowrap;
  margin-top: 3px; /* 与正文首行文字视觉中轴对齐 */
}

/* 身份前缀列：固定 4.8em 宽度；chip 靠右紧贴 LV，无前缀行整列留空 → LV 各行同列对齐 */
.role-slot {
  display: inline-flex;
  align-items: center;
  justify-content: flex-end;
  gap: 0.2em;
  width: 4.8em;
  margin-right: 0.35em;
  overflow: hidden;
}

/* 前缀列内 chip 间距由 gap 控制，去掉全局右 margin（否则尾 chip 与 LV 间出现空隙） */
.role-slot .chip {
  margin-right: 0;
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
}

/* 粉丝牌徽章：较短牌名与“名+两位数”同宽 */
.chip.medal-room,
.chip.medal-other {
  min-width: 7.2ch;
}

/* 单字身份标记（房/舰）方形居中 */
.chip.role {
  min-width: 2.6ch;
}

/* 荣耀等级 */
.chip.lv {
  color: #6b4a00;
  background: linear-gradient(180deg, #ffe08a, #f0b429);
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

/* 身份标记：舰长（蓝）/提督（紫）/总督（金红）三档配色，文字保持统一白色 */
.chip.role {
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
