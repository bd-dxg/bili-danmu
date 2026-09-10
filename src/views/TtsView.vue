<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import DanmakuFilterPanel from "../components/DanmakuFilterPanel.vue";
import {
  DEFAULT_DANMAKU_FILTER,
  DEFAULT_TTS_CONFIG,
  type DanmakuFilter,
  type TtsConfig,
  type TtsVoice,
} from "../types/ipc";

// 朗读配置：与弹幕显示完全解耦，改动即时生效（下一段朗读开始用新参数）
const config = ref<TtsConfig>({
  ...DEFAULT_TTS_CONFIG,
  filter: { ...DEFAULT_DANMAKU_FILTER },
});
const voices = ref<TtsVoice[]>([]);
const savedTip = ref(false);
const opError = ref("");
// 试听按钮防连点（合成 + 播放需要一两秒）
const testing = ref(false);
// 音色列表拉取中（微软 voices/list 有 300+ 条，首次进入页面后台拉取）
const loadingVoices = ref(false);
// 页内标签：engine / content / filter
const tab = ref<"engine" | "content" | "filter">("engine");

// 配置里的音色不在列表中（旧版手填的残值 / 非中文音色）：补一项展示，避免下拉框空白
const customVoice = computed(() =>
  voices.value.length > 0 && !voices.value.some((v) => v.id === config.value.voice)
    ? config.value.voice
    : "",
);

let tipTimer: ReturnType<typeof setTimeout> | undefined;

function showTip() {
  opError.value = "";
  savedTip.value = true;
  if (tipTimer) clearTimeout(tipTimer);
  tipTimer = setTimeout(() => (savedTip.value = false), 1200);
}

function showError(e: unknown) {
  savedTip.value = false;
  opError.value = String(e);
  if (tipTimer) clearTimeout(tipTimer);
  tipTimer = setTimeout(() => (opError.value = ""), 3000);
}

async function save() {
  try {
    // 数字输入框清空时 v-model.number 为 ''，避免脏值传给 Rust u32 反序列化报错
    if (!Number.isFinite(config.value.max_len)) {
      config.value.max_len = DEFAULT_TTS_CONFIG.max_len;
    }
    if (!Number.isFinite(config.value.max_queue)) {
      config.value.max_queue = DEFAULT_TTS_CONFIG.max_queue;
    }
    await invoke("tts_set_config", { tts: { ...config.value } });
    showTip();
  } catch (e) {
    showError(e);
  }
}

function applyFilter(next: DanmakuFilter) {
  config.value.filter = next;
  save();
}

async function testSpeak() {
  testing.value = true;
  try {
    // 先落盘再试听，保证听到的是刚选的音色 / 语速 / 音量
    await save();
    await invoke("tts_test_speak", { text: null });
  } catch (e) {
    showError(e);
  } finally {
    setTimeout(() => (testing.value = false), 1500);
  }
}

async function refreshVoices() {
  loadingVoices.value = true;
  try {
    const list = await invoke<TtsVoice[]>("tts_refresh_voices");
    if (list.length) voices.value = list;
  } catch (e) {
    // 拉取失败保留内置中文音色，音色名仍可手填
    console.error("刷新音色列表失败", e);
  } finally {
    loadingVoices.value = false;
  }
}

onMounted(async () => {
  try {
    config.value = await invoke<TtsConfig>("tts_get_config");
  } catch (e) {
    console.error("读取朗读配置失败", e);
  }
  // 先上内置列表秒开，再后台换成微软完整列表
  try {
    voices.value = await invoke<TtsVoice[]>("tts_list_voices");
  } catch (e) {
    console.error("读取音色列表失败", e);
  }
  await fixInvalidVoice();
  refreshVoices();
});

/** 配置里的音色不在列表中时静默回退默认音色：否则每一条弹幕合成都拿不到音频 */
async function fixInvalidVoice() {
  if (!voices.value.length) return;
  if (voices.value.some((v) => v.id === config.value.voice)) return;
  console.warn(`音色 ${config.value.voice} 不在列表中，已回退默认音色`);
  config.value.voice = DEFAULT_TTS_CONFIG.voice;
  try {
    await invoke("tts_set_config", { tts: { ...config.value } });
  } catch (e) {
    console.error("回退音色失败", e);
  }
}
</script>

<template>
  <div class="tts-page">
    <!-- 保存/错误提示常驻在标签栏上方，切标签也能看到 -->
    <p v-if="opError" class="status error">{{ opError }}</p>
    <p v-else-if="savedTip" class="status saved">✓ 已应用并保存</p>

    <div class="tabs">
      <button
        v-for="t in [
          { key: 'engine', label: '引擎与音色' },
          { key: 'content', label: '朗读内容' },
          { key: 'filter', label: '朗读筛选' },
        ]"
        :key="t.key"
        class="tab-btn"
        :class="{ active: tab === t.key }"
        @click="tab = t.key as 'engine' | 'content' | 'filter'"
      >
        {{ t.label }}
      </button>
    </div>

    <section v-show="tab === 'engine'" class="tab-pane">

    <div class="setting-card">
      <div class="setting-row">
        <span class="label">开启弹幕朗读</span>
        <input
          v-model="config.enabled"
          type="checkbox"
          class="switch"
          @change="save()"
        />
      </div>

      <div class="setting-row">
        <span class="label">音色</span>
        <div class="voice-control">
          <select v-model="config.voice" class="select" @change="save()">
            <option v-for="v in voices" :key="v.id" :value="v.id">
              {{ v.label }}
            </option>
            <option v-if="customVoice" :value="customVoice">
              {{ customVoice }}（无效音色，请重新选择）
            </option>
          </select>
          <button
            class="ghost-btn"
            :disabled="loadingVoices"
            @click="refreshVoices()"
          >
            {{ loadingVoices ? "获取中…" : "刷新" }}
          </button>
        </div>
      </div>

      <div class="setting-row">
        <span class="label">语速</span>
        <div class="range-control">
          <input
            v-model.number="config.rate_pct"
            type="range"
            min="-50"
            max="100"
            step="5"
            @change="save()"
          />
          <span class="value">{{ (1 + config.rate_pct / 100).toFixed(2) }}x</span>
        </div>
      </div>

      <div class="setting-row">
        <span class="label">音量</span>
        <div class="range-control">
          <input
            v-model.number="config.volume_pct"
            type="range"
            min="-100"
            max="100"
            step="5"
            @change="save()"
          />
          <span class="value">{{ config.volume_pct > 0 ? "+" : "" }}{{ config.volume_pct }}%</span>
        </div>
      </div>

      <div class="setting-row">
        <span class="label">试听当前设置</span>
        <button class="save-btn" :disabled="testing" @click="testSpeak()">
          {{ testing ? "朗读中…" : "试听" }}
        </button>
      </div>

      <p class="tip">
        使用微软 Edge 的在线语音（Edge TTS），共 {{ voices.length }} 个中文音色，
        普通话排在最前，其后是方言与粤语 / 台湾。当前音色 ID：{{ config.voice }}。
        合成依赖网络，断网或微软限流时这一段会跳过，不影响下一条。
      </p>
    </div>

    </section>

    <section v-show="tab === 'content'" class="tab-pane">

    <div class="setting-card">
      <div class="setting-row">
        <span class="label">朗读用户名</span>
        <input
          v-model="config.read_username"
          type="checkbox"
          class="switch"
          @change="save()"
        />
      </div>

      <div class="setting-row">
        <span class="label">朗读身份前缀（房管 / 舰长）</span>
        <input
          v-model="config.read_role"
          type="checkbox"
          class="switch"
          @change="save()"
        />
      </div>

      <div class="setting-row">
        <span class="label">积压时打断当前朗读</span>
        <input
          v-model="config.interrupt_on_backlog"
          type="checkbox"
          class="switch"
          @change="save()"
        />
      </div>

      <div class="setting-row">
        <span class="label">弹幕内容最大朗读字数（0 = 不限制）</span>
        <input
          v-model.number="config.max_len"
          type="number"
          min="0"
          max="500"
          class="num"
          @change="save()"
        />
      </div>

      <div class="setting-row">
        <span class="label">待朗读队列上限（超出丢弃最旧）</span>
        <input
          v-model.number="config.max_queue"
          type="number"
          min="1"
          max="200"
          class="num"
          @change="save()"
        />
      </div>

      <p class="tip">
        关掉用户名与身份前缀 = 只念弹幕内容（默认）。最大字数只算弹幕正文，
        身份前缀与用户名不占额度。高速直播间建议同时开「积压时打断」：
        新弹幕会直接顶掉正在念的那条，延迟上限压到一条朗读时长（每条至少念 1.8 秒才可能被打断）。
        队列上限调小则能保证念的都是最新弹幕。
      </p>
    </div>

    </section>

    <section v-show="tab === 'filter'" class="tab-pane">

    <p class="tip">
      这里的规则只影响朗读，与「弹幕 → 弹幕过滤」的显示筛选各自独立。
    </p>
    <DanmakuFilterPanel
      :model-value="config.filter"
      verb="朗读"
      @change="applyFilter"
    />

    </section>
  </div>
</template>

<style scoped>
.tts-page {
  display: flex;
  flex-direction: column;
}

/* 页内标签栏 */
.tabs {
  display: flex;
  gap: 2px;
  border-bottom: 1px solid var(--border);
  margin-bottom: 14px;
}

.tab-btn {
  background: none;
  border: none;
  color: var(--text-dim);
  font-size: 17px;
  font-weight: bold;
  padding: 8px 16px 10px;
  border-bottom: 2px solid transparent;
  margin-bottom: -1px;
  cursor: pointer;
}

.tab-btn:hover {
  color: var(--text);
}

.tab-btn.active {
  color: var(--accent);
  border-bottom-color: var(--accent);
  font-weight: 600;
}

.setting-card {
  background: var(--bg-side);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 4px 14px;
}

.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 0;
  border-bottom: 1px solid var(--border);
  font-size: 16px;
}

.setting-row:last-of-type {
  border-bottom: none;
}

.switch {
  width: 17px;
  height: 17px;
  accent-color: var(--accent);
}

.select {
  background: var(--bg-elev);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  padding: 6px 10px;
  font-size: 15px;
  outline: none;
  min-width: 220px;
}

.select:focus {
  border-color: var(--accent);
}

.voice-control {
  display: flex;
  align-items: center;
  gap: 8px;
}

.ghost-btn {
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 6px 12px;
  font-size: 14px;
  color: var(--text-dim);
  background: none;
  cursor: pointer;
  white-space: nowrap;
}

.ghost-btn:hover:not(:disabled) {
  color: var(--text);
  border-color: var(--accent);
}

.ghost-btn:disabled {
  opacity: 0.6;
  cursor: default;
}

.range-control {
  display: flex;
  align-items: center;
  gap: 10px;
}

.range-control input[type="range"] {
  width: 160px;
  accent-color: var(--accent);
}

.value {
  min-width: 52px;
  text-align: right;
  color: var(--text-dim);
  font-size: 15px;
}

.num {
  width: 70px;
  background: var(--bg-elev);
  border: 1px solid var(--border);
  border-radius: 5px;
  color: var(--text);
  padding: 4px 8px;
  font-size: 15px;
  outline: none;
  text-align: center;
}

.num:focus {
  border-color: var(--accent);
}

.tip {
  font-size: 14px;
  color: var(--text-faint);
  padding: 4px 0 10px;
  line-height: 1.6;
}

.save-btn {
  border: none;
  border-radius: 6px;
  padding: 6px 18px;
  font-size: 15px;
  color: #fff;
  background: var(--accent);
  cursor: pointer;
}

.save-btn:hover:not(:disabled) {
  background: var(--accent-hover);
}

.save-btn:disabled {
  opacity: 0.6;
  cursor: default;
}

.status {
  font-size: 14px;
  margin-bottom: 8px;
}

.error {
  color: var(--red);
}

.saved {
  color: var(--green);
}
</style>
