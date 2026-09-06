<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  DEFAULT_DANMAKU_FILTER,
  type DanmakuEvent,
  type DanmakuFilter,
  type OverlayStyle,
  type RoomStatusEvent,
} from "../types/ipc";

// 弹幕列表（统一行样式：徽章同尺寸、文字同字号）
interface DisplayDanmaku extends DanmakuEvent {
  isRoomMedal: boolean;
}

const danmakuList = ref<DisplayDanmaku[]>([]);
const currentRoomId = ref(0);
// 连接状态（空窗时显示初始化提示，避免无界面窗口）
const connState = ref<"disconnected" | "connected">("disconnected");
// 弹幕过滤配置（设置页变更 → Rust 广播 danmaku-filter 事件 → 实时生效）
const filter = ref<DanmakuFilter>({ ...DEFAULT_DANMAKU_FILTER });
// 弹幕样式（字号/字体/是否用原色）
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

/** 描边 → 四向 text-shadow（硬边，宽度近似） */
const rowShadow = computed(() => {
  const s = style.value;
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
const MAX_ITEMS = 120;

/** 连接状态 → 界面状态（事件与轮询共用，去重） */
function applyConnStatus(st: RoomStatusEvent) {
  if (st.state === "connected") {
    const was = connState.value === "connected";
    const prevRoom = currentRoomId.value;
    connState.value = "connected";
    currentRoomId.value = st.roomId;
    // 进入（或换房）时：清空旧提示并以系统行提示入场
    if (!was || prevRoom !== st.roomId) {
      danmakuList.value = [];
      pushSystem(`已进入直播间 ${st.roomId}，等待弹幕…`);
    }
  } else if (st.state === "connecting") {
    if (connState.value !== "connecting") {
      connState.value = "connecting";
      danmakuList.value = [];
      pushSystem("连接中…");
    }
  } else {
    if (connState.value !== "disconnected") {
      connState.value = "disconnected";
      danmakuList.value = [];
      pushSystem("未连接 · 请在主界面连接直播间");
    }
  }
}

/** 推送系统提示行（无用户名，纯文本，与弹幕同样式） */
function pushSystem(text: string) {
  const list = danmakuList.value;
  list.push({
    id: `sys-${Date.now()}-${list.length}`,
    username: "",
    content: text,
    timestamp: Date.now() / 1000,
    is_admin: false,
  });
  if (list.length > MAX_ITEMS) {
    list.splice(0, list.length - MAX_ITEMS);
  }
}

let unlistenDanmu: UnlistenFn | undefined;
let unlistenRoom: UnlistenFn | undefined;
let unlistenStyle: UnlistenFn | undefined;
let unlistenFilter: UnlistenFn | undefined;

/// 弹幕过滤判定：
/// 身份规则（舰长/房管、有粉丝牌、荣耀等级）为「或」关系——任一开启的规则命中即显示；
/// 身份规则全关 = 不过滤；敏感词屏蔽独立叠加——开关开启且命中词表时整条丢弃（不豁免）。
function shouldShowDanmaku(d: DanmakuEvent, f: DanmakuFilter): boolean {
  if (
    f.enable_sensitive &&
    f.sensitive_words.some((w) => w && d.content.toLowerCase().includes(w.toLowerCase()))
  ) {
    return false;
  }
  const anyIdentityOn =
    f.enable_guard_admin || f.enable_medal || f.enable_wealth;
  if (!anyIdentityOn) return true;
  return (
    (f.enable_guard_admin && (d.is_admin || (d.guard_level ?? 0) >= 1)) ||
    (f.enable_medal && (d.medal_level ?? 0) > 0) ||
    (f.enable_wealth && (d.wealth_level ?? 0) >= f.wealth_min)
  );
}

/// 身份前缀列表（可叠加）：房管 + 舰/提督/总督，按展示顺序
function rolesOf(d: DisplayDanmaku): { label: string; cls: string }[] {
  const roles: { label: string; cls: string }[] = [];
  if (d.is_admin) {
    roles.push({ label: "房", cls: "admin" });
  }
  if (d.guard_level && d.guard_level >= 1) {
    roles.push({
      label: d.guard_level === 3 ? "舰" : d.guard_level === 2 ? "提督" : "总督",
      cls: "guard",
    });
  }
  return roles;
}

onMounted(async () => {
  unlistenRoom = await listen<RoomStatusEvent>("room-status", (e) => {
    applyConnStatus(e.payload);
  });
  unlistenDanmu = await listen<DanmakuEvent>("danmaku", (e) => {
    const d = e.payload as DisplayDanmaku;
    // 过滤：不满足配置规则（敏感词命中/身份不匹配）的弹幕整条丢弃
    if (!shouldShowDanmaku(d, filter.value)) {
      return;
    }
    d.isRoomMedal =
      d.medal_room_id !== undefined &&
      currentRoomId.value !== 0 &&
      d.medal_room_id === currentRoomId.value;
    const list = danmakuList.value;
    list.push(d);
    if (list.length > MAX_ITEMS) {
      list.splice(0, list.length - MAX_ITEMS);
    }
  });
  unlistenStyle = await listen<OverlayStyle>("overlay-style", (e) => {
    style.value = e.payload;
  });
  unlistenFilter = await listen<DanmakuFilter>("danmaku-filter", (e) => {
    filter.value = e.payload;
  });
  // 初始同步样式
  try {
    style.value = await invoke<OverlayStyle>("overlay_get_style");
  } catch {
    /* ignore */
  }
  // 初始同步过滤配置
  try {
    filter.value = await invoke<DanmakuFilter>("danmaku_get_filter");
  } catch {
    /* ignore */
  }
  // 轮询兑底：事件丢失时也能同步连接状态（每 2s）
  const poll = async () => {
    try {
      const st = await invoke<RoomStatusEvent>("get_connection_status");
      applyConnStatus(st);
    } catch {
      /* overlay 独立打开等场景忽略 */
    }
  };
  await poll();
  const timer = setInterval(poll, 2000);
  onUnmounted(() => clearInterval(timer));
});

onUnmounted(() => {
  unlistenDanmu?.();
  unlistenRoom?.();
  unlistenStyle?.();
  unlistenFilter?.();
});
</script>

<template>
  <div
    class="overlay-root"
    :style="{
      fontSize: style.font_size + 'px',
      fontFamily: style.font_family,
      rowGap: style.row_gap + 'px',
    }"
    data-tauri-drag-region
  >
    <!-- 常驻透明拖拽条：不在弹幕流内，弹幕高速刷新时也能稳定拖动窗口 -->
    <div class="drag-strip" data-tauri-drag-region></div>
    <div
      v-for="d in danmakuList"
      :key="d.id"
      class="danmu-row"
      :style="{ textShadow: rowShadow }"
      data-tauri-drag-region
    >
      <!-- 徽章区：固定前缀列 + LV/粉丝牌等，单行不折行 -->
      <span class="meta">
        <!-- 身份前缀列（固定宽度，无前缀留空）→ LV/粉丝牌/文字各行垂直对齐 -->
        <span class="role-slot">
          <template v-if="style.show_role">
            <span
              v-for="r in rolesOf(d)"
              :key="r.cls"
              class="chip role"
              :class="r.cls"
            >{{ r.label }}</span>
          </template>
        </span>
        <template v-if="style.show_wealth && d.wealth_level && d.wealth_level > 0">
          <span class="chip lv">LV{{ d.wealth_level }}</span>
        </template>
        <template v-if="style.show_medal && d.medal_name && d.isRoomMedal">
          <span class="chip medal-room">
            {{ d.medal_name }}{{ d.medal_level ?? 0 }}
          </span>
        </template>
        <template v-else-if="style.show_medal && d.medal_name">
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
              color: style.username_color,
              fontWeight: style.bold ? 800 : 600,
            }"
          >{{ d.username }}：</span>
        </template>
        <span
          class="content"
          :style="{
            color: style.content_color,
            fontWeight: style.bold ? 700 : 400,
          }"
        >{{ d.content }}</span>
      </span>
    </div>
  </div>
</template>

<style scoped>
.overlay-root {
  position: relative;
  height: 100%;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  padding: 6px;
  box-sizing: border-box;
  overflow: hidden;
  background: transparent;
  user-select: none;
}

/* 顶部常驻拖拽条：平时透明不遮挡弹幕；鼠标移入悬浮窗即浮出提示（无需精确悬停到条上） */
.drag-strip {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 16px;
  z-index: 5;
  cursor: move;
  background: transparent;
  border-bottom: 1px solid transparent;
  transition:
    background 0.12s ease,
    border-color 0.12s ease;
}

.overlay-root:hover .drag-strip {
  background: rgba(255, 255, 255, 0.1);
  border-bottom-color: rgba(255, 255, 255, 0.35);
}

/* 身份前缀列：固定 4em 宽度；chip 靠右紧贴 LV，无前缀行整列留空 → LV 各行同列对齐 */
.role-slot {
  display: inline-flex;
  align-items: center;
  justify-content: flex-end;
  gap: 0.2em;
  width: 4em;
  margin-right: 0.35em;
  overflow: hidden;
}

/* 前缀列内 chip 间距由 gap 控制，去掉全局右 margin（否则尾 chip 与 LV 间出现空隙） */
.role-slot .chip {
  margin-right: 0;
}

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

/* 本房粉丝牌 */
.chip.medal-room {
  color: #fff;
  background: linear-gradient(180deg, #b06ff0, #7c4dd4);
}

/* 其它房粉丝牌 */
.chip.medal-other {
  color: #d6d9de;
  background: rgba(80, 80, 90, 0.9);
}

/* 身份标记：舰长（金）/房管（蓝）小色块；文字保持统一白色 */
.chip.role {
  color: #fff;
}

.chip.role.guard {
  background: linear-gradient(180deg, #ffd75e, #c9961c);
  color: #3a2c00;
}

.chip.role.admin {
  background: linear-gradient(180deg, #3aa0ff, #1d6fd6);
}
</style>
