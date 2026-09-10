<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import DanmakuRow from "./DanmakuRow.vue";
import {
  DEFAULT_DANMAKU_FILTER,
  type DanmakuEvent,
  type DanmakuFilter,
  type DisplayDanmaku,
  type OverlayStyle,
  type RoomStatusEvent,
} from "../types/ipc";

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
    isRoomMedal: false,
  });
  if (list.length > MAX_ITEMS) {
    list.splice(0, list.length - MAX_ITEMS);
  }
}

/** 追加一条弹幕，超出上限丢弃最旧的 */
function pushDanmaku(d: DisplayDanmaku) {
  const list = danmakuList.value;
  list.push(d);
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
    pushDanmaku(d);
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
  // 强制触发字体渲染重计算，避免首次加载时字体像素化
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
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
    <!-- 常驻透明拖拽条：不在弹幕流内，弹幕高速刷新时也能稳定拖动窗口；移入时显示操作提示 -->
    <div class="drag-strip" data-tauri-drag-region>
      <span class="drag-hint">按住此区域可拖动弹幕窗位置</span>
    </div>
    <DanmakuRow
      v-for="d in danmakuList"
      :key="d.id"
      :d="d"
      :overlay-style="style"
    />
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
  /* 左端对齐正文列（跳过身份前缀列 role-slot 4.8em + 0.35em 间距），不覆盖房管/舰长徽章 */
  left: calc(6px + 5.15em);
  right: 0;
  height: 1.4em;
  z-index: 5;
  cursor: move;
  background: transparent;
  border-bottom: 1px solid transparent;
  display: flex;
  align-items: center;
  justify-content: center;
  overflow: hidden;
  transition:
    background 0.12s ease,
    border-color 0.12s ease;
}

/* 操作提示：默认淡出，鼠标移入悬浮窗时显示；pointer-events 穿透保证整条可拖动 */
.drag-hint {
  font-size: 0.8em;
  line-height: 1;
  color: rgba(255, 255, 255, 0.92);
  opacity: 0;
  pointer-events: none;
  white-space: nowrap;
  transition: opacity 0.12s ease;
}

.overlay-root:hover .drag-strip {
  background: rgba(0, 0, 0, 0.55);
  border-bottom-color: rgba(255, 255, 255, 0.35);
}

.overlay-root:hover .drag-hint {
  opacity: 1;
}
</style>
