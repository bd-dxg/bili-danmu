<script setup lang="ts">
import { ref } from "vue";
import DanmakuFilterPanel from "../components/DanmakuFilterPanel.vue";
import SettingRow from "../components/SettingRow.vue";
import { useSaveTip } from "../composables/useSaveTip";
import { useTtsConfig } from "../composables/useTtsConfig";
import type { DanmakuFilter } from "../types/ipc";

// 页内标签：engine / content / filter
const tab = ref<"engine" | "content" | "filter">("engine");

const { savedTip, opError, showSaved, showError } = useSaveTip();
// 朗读配置与音色列表：与弹幕显示完全解耦，改动即时生效（下一段朗读开始用新参数）
const {
  config,
  voices,
  testing,
  loadingVoices,
  customVoice,
  save,
  testSpeak,
  refreshVoices,
} = useTtsConfig(showSaved, showError);

function applyFilter(next: DanmakuFilter) {
  config.value.filter = next;
  save();
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
        <SettingRow label="开启弹幕朗读">
          <input
            v-model="config.enabled"
            type="checkbox"
            class="switch"
            @change="save()"
          />
        </SettingRow>

        <SettingRow label="音色">
          <div class="voice-control">
            <select v-model="config.voice" class="select" @change="save()">
              <option v-for="v in voices" :key="v.id" :value="v.id">
                {{ v.label }}
              </option>
              <option v-if="customVoice" :value="customVoice">
                {{ customVoice }}（不在列表中，可点「刷新」重试）
              </option>
            </select>
            <button class="ghost-btn" :disabled="loadingVoices" @click="refreshVoices()">
              {{ loadingVoices ? "获取中…" : "刷新" }}
            </button>
          </div>
        </SettingRow>

        <SettingRow label="语速">
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
        </SettingRow>

        <SettingRow label="音量">
          <div class="range-control">
            <input
              v-model.number="config.volume_pct"
              type="range"
              min="-100"
              max="100"
              step="5"
              @change="save()"
            />
            <span class="value"
              >{{ config.volume_pct > 0 ? "+" : "" }}{{ config.volume_pct }}%</span
            >
          </div>
        </SettingRow>

        <SettingRow label="试听当前设置">
          <button class="save-btn" :disabled="testing" @click="testSpeak()">
            {{ testing ? "朗读中…" : "试听" }}
          </button>
        </SettingRow>

        <p class="tip">
          使用微软 Edge 的在线语音（Edge TTS），共 {{ voices.length }} 个中文音色，
          普通话排在最前，其后是方言与粤语 / 台湾。当前音色 ID：{{ config.voice }}。
          合成依赖网络，断网或微软限流时这一段会跳过，不影响下一条。
        </p>
      </div>
    </section>

    <section v-show="tab === 'content'" class="tab-pane">
      <div class="setting-card">
        <SettingRow label="朗读用户名">
          <input
            v-model="config.read_username"
            type="checkbox"
            class="switch"
            @change="save()"
          />
        </SettingRow>

        <SettingRow label="朗读身份前缀（房管 / 舰长）">
          <input
            v-model="config.read_role"
            type="checkbox"
            class="switch"
            @change="save()"
          />
        </SettingRow>

        <SettingRow label="积压时丢弃待播旧弹幕（当前这条念完）">
          <input
            v-model="config.interrupt_on_backlog"
            type="checkbox"
            class="switch"
            @change="save()"
          />
        </SettingRow>

        <SettingRow label="弹幕内容最大朗读字数（0 = 不限制）">
          <input
            v-model.number="config.max_len"
            type="number"
            min="0"
            max="500"
            class="num"
            @change="save()"
          />
        </SettingRow>

        <SettingRow label="待朗读队列上限（超出丢弃最旧）">
          <input
            v-model.number="config.max_queue"
            type="number"
            min="1"
            max="200"
            class="num"
            @change="save()"
          />
        </SettingRow>

        <p class="tip">
          关掉用户名与身份前缀 = 只念弹幕内容（默认）。最大字数只算弹幕正文，
          身份前缀与用户名不占额度。高速直播间建议同时开「积压时丢弃旧弹幕」：
          念的总是队列里最新的那条，正在念的那条不被打断（避免只听到半句用户名），
          延迟上限 = 一条朗读时长。队列上限调小则能保证念的都是最新弹幕。
        </p>
      </div>
    </section>

    <section v-show="tab === 'filter'" class="tab-pane">
      <p class="tip">
        这里的规则只影响朗读，与「弹幕 → 弹幕过滤」的显示筛选各自独立。
      </p>
      <DanmakuFilterPanel :model-value="config.filter" verb="朗读" @change="applyFilter" />
    </section>
  </div>
</template>

<style scoped>
.tts-page {
  display: flex;
  flex-direction: column;
}

/* 设置卡片 / 标签栏 / 开关 / 下拉 / 提示 / 主按钮见全局 src/styles/settings.css */
.select {
  min-width: 220px;
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
