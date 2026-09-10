<script setup lang="ts">
// 状态仪表盘：只读展示弹幕窗/朗读/速率状态，按秒轮询 Rust 聚合命令
// （开关本身分散在「弹幕」「朗读」页，这里不提供操作入口，避免两份状态不同步）
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { DanmakuFilter, DashboardStatus } from "../types/ipc";

const status = ref<DashboardStatus | null>(null);
let timer: ReturnType<typeof setInterval> | undefined;

async function refresh() {
  try {
    status.value = await invoke<DashboardStatus>("get_dashboard_status");
  } catch (e) {
    console.error("读取仪表盘状态失败", e);
  }
}

onMounted(() => {
  void refresh();
  timer = setInterval(refresh, 1000);
});

onUnmounted(() => {
  if (timer) clearInterval(timer);
});

// 朗读条件人话摘要：身份规则之间是「或」，敏感词屏蔽独立叠加，全关 = 不限身份
function filterSummary(f: DanmakuFilter): string {
  const roles: string[] = [];
  if (f.enable_guard_admin) roles.push("舰长/房管");
  if (f.enable_medal) roles.push("粉丝牌");
  if (f.enable_wealth) roles.push(`荣耀 ≥ ${f.wealth_min}`);
  const base = roles.length ? `只读 ${roles.join(" / ")}` : "全部弹幕";
  const words = f.enable_sensitive
    ? f.sensitive_words.filter((w) => w.trim() !== "").length
    : 0;
  return words > 0 ? `${base}，屏蔽 ${words} 词` : base;
}
</script>

<template>
  <div class="dashboard">
    <h3>运行状态</h3>
    <div class="cards">
      <div class="card">
        <div class="card-label">弹幕速率</div>
        <div
          class="card-value rate"
          :class="{ live: (status?.danmaku_rate ?? 0) > 0 }"
        >
          {{ status ? status.danmaku_rate : "—" }}
        </div>
        <div class="card-sub">条 / 10 秒</div>
      </div>

      <div class="card">
        <div class="card-label">弹幕窗</div>
        <div class="card-value" :class="status?.overlay.visible ? 'on' : 'off'">
          {{ status ? (status.overlay.visible ? "显示中" : "已隐藏") : "—" }}
        </div>
        <div class="card-sub">
          穿透{{ status?.overlay.clickthrough ? "开" : "关" }} · 置顶{{
            status?.overlay.always_on_top ? "开" : "关"
          }}
        </div>
      </div>

      <div class="card">
        <div class="card-label">朗读</div>
        <div class="card-value" :class="status?.tts.enabled ? 'on' : 'off'">
          {{ status ? (status.tts.enabled ? "已开启" : "已关闭") : "—" }}
        </div>
        <div class="card-sub">
          {{ status && status.tts.enabled ? filterSummary(status.tts.filter) : "—" }}
        </div>
      </div>
    </div>
    <p class="tip">只读展示；修改请到「弹幕」或「朗读」页。</p>
  </div>
</template>

<style scoped>
.dashboard {
  margin-top: 22px;
}

h3 {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-dim);
  margin-bottom: 10px;
}

/* 窗口变窄时自动折行，避免三列被压扁 */
.cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
  gap: 10px;
}

.card {
  background: var(--bg-side);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 10px 12px 12px;
  min-width: 0;
}

.card-label {
  font-size: 13px;
  color: var(--text-faint);
  margin-bottom: 6px;
}

.card-value {
  font-size: 17px;
  font-weight: 600;
  color: var(--text);
  line-height: 1.2;
}

/* 速率是这块的视觉重心：大号 + 等宽数字，跳动时宽度不抖 */
.card-value.rate {
  font-size: 30px;
  font-variant-numeric: tabular-nums;
  letter-spacing: -0.5px;
}

.card-value.on {
  color: var(--green);
}

.card-value.off {
  color: var(--text-faint);
}

.card-value.live {
  color: var(--accent);
}

.card-sub {
  font-size: 12px;
  color: var(--text-dim);
  margin-top: 4px;
  line-height: 1.4;
  overflow-wrap: anywhere;
}

.tip {
  font-size: 13px;
  color: var(--text-faint);
  padding-top: 8px;
}
</style>
