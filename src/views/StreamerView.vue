<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import SettingRow from "../components/SettingRow.vue";
import { useSaveTip } from "../composables/useSaveTip";
import { DEFAULT_GIFT_CONFIG, DEFAULT_GIFT_TTS_CONFIG, type GiftConfig, type GiftTtsConfig } from "../types/ipc";

// 页内标签：gift / giftTts / welcome
const tab = ref<"gift" | "giftTts" | "welcome">("gift");

// 礼物列表配置：金额门槛与连击合并都在 Rust 侧判定，这里只负责读写与展示
const gift = ref<GiftConfig>({ ...DEFAULT_GIFT_CONFIG });
// 礼物朗读配置：开关与门槛独立于弹幕朗读与礼物区显示
const giftTts = ref<GiftTtsConfig>({ ...DEFAULT_GIFT_TTS_CONFIG });

const { savedTip, opError, showSaved, showError } = useSaveTip();

async function apply() {
  // 数字输入框可能被清空或填成负数，统一收敛到 0 再下发（0 = 不限）
  const amount = Number(gift.value.min_amount_yuan);
  gift.value.min_amount_yuan = Number.isFinite(amount) && amount > 0 ? amount : 0;
  try {
    await invoke("gift_set_config", { gift: { ...gift.value } });
    showSaved();
  } catch (e) {
    showError(e);
  }
}

async function applyGiftTts() {
  // 同礼物区：数字输入框可能被清空或填成负数，统一收敛到 0 再下发（0 = 不限）
  const amount = Number(giftTts.value.min_amount_yuan);
  giftTts.value.min_amount_yuan = Number.isFinite(amount) && amount > 0 ? amount : 0;
  try {
    await invoke("gift_tts_set_config", { giftTts: { ...giftTts.value } });
    showSaved();
  } catch (e) {
    showError(e);
  }
}

onMounted(async () => {
  try {
    gift.value = await invoke<GiftConfig>("gift_get_config");
    giftTts.value = await invoke<GiftTtsConfig>("gift_tts_get_config");
  } catch (e) {
    console.error("读取礼物配置失败", e);
  }
});
</script>

<template>
  <div class="streamer-page">
    <p v-if="opError" class="error op-error">{{ opError }}</p>
    <div class="tabs">
      <button
        v-for="t in [
          { key: 'gift', label: '礼物渲染' },
          { key: 'giftTts', label: '礼物朗读' },
          { key: 'welcome', label: '欢迎信息' },
        ]"
        :key="t.key"
        class="tab-btn"
        :class="{ active: tab === t.key }"
        @click="tab = t.key as 'gift' | 'giftTts' | 'welcome'"
      >
        {{ t.label }}
      </button>
    </div>

    <section v-show="tab === 'gift'" class="tab-pane">
      <div class="setting-card">
        <SettingRow label="在弹幕窗显示礼物区">
          <input
            v-model="gift.enabled"
            type="checkbox"
            class="switch"
            @change="apply()"
          />
        </SettingRow>

        <SettingRow label="金额门槛">
          <div class="num-control">
            <input
              v-model.number="gift.min_amount_yuan"
              type="number"
              min="0"
              step="1"
              class="num"
              @change="apply()"
            />
            <span class="unit">元（0 = 不限）</span>
          </div>
        </SettingRow>

        <SettingRow label="最多显示条数">
          <div class="size-control">
            <input
              v-model.number="gift.max_rows"
              type="range"
              min="1"
              max="20"
              step="1"
              @change="apply()"
            />
            <span class="value">{{ gift.max_rows }} 行</span>
          </div>
        </SettingRow>

        <SettingRow label="连击合并窗口">
          <div class="size-control">
            <input
              v-model.number="gift.combo_window_secs"
              type="range"
              min="1"
              max="30"
              step="1"
              @change="apply()"
            />
            <span class="value">{{ gift.combo_window_secs }} 秒</span>
          </div>
        </SettingRow>

        <p class="tip">
          只收录付费打赏（普通礼物 / 醒目留言 / 上舰），银瓜子等免费礼物不进列表。
          金额以人民币计，连击按累加后的总额判定：门槛设 30 元时，连送 30 个 1 元礼物会攒够才出现。
          同一观众同一种礼物在窗口内的多次送出合并成一行，数量与金额累加、位置不变。
          礼物行跟随弹幕样式（字号 / 描边 / 行间距），礼物名与金额用金色区分。
        </p>
        <p v-if="savedTip" class="saved">✓ 已应用并保存</p>
      </div>
    </section>

    <section v-show="tab === 'giftTts'" class="tab-pane">
      <div class="setting-card">
        <SettingRow label="朗读打赏（礼物 / 醒目留言 / 上舰）">
          <input
            v-model="giftTts.enabled"
            type="checkbox"
            class="switch"
            @change="applyGiftTts()"
          />
        </SettingRow>

        <SettingRow label="朗读金额门槛">
          <div class="num-control">
            <input
              v-model.number="giftTts.min_amount_yuan"
              type="number"
              min="0"
              step="1"
              class="num"
              @change="applyGiftTts()"
            />
            <span class="unit">元（0 = 不限）</span>
          </div>
        </SettingRow>

        <p class="tip">
          打赏朗读会插队：排到所有待朗读弹幕之前（正在念的那条念完再接，不掐断半句）。
          文案为「感谢老板A送的5个辣条」，醒目留言念留言正文，上舰念「感谢老板A上舰舰长」。
          连击按「礼物渲染」标签里的连击合并窗口聚合，窗口结束后只念一次汇总；
          门槛按累加后的总额判定，设 30 元时连送 30 个 1 元礼物会攒够才念。
          开关独立于弹幕朗读：关掉「朗读 → 开启弹幕朗读」后，打赏照念。
        </p>
        <p v-if="savedTip" class="saved">✓ 已应用并保存</p>
      </div>
    </section>

    <section v-show="tab === 'welcome'" class="tab-pane">
      <div class="setting-card">
        <p class="tip">
          进房观众 / 上舰 / 关注点赞的欢迎消息：渲染开关、朗读开关、过滤规则（如仅舰长以上、
          忽略无粉丝牌用户）。功能未实现。
        </p>
      </div>
    </section>
  </div>
</template>

<style scoped>
/* 设置卡片 / 标签栏 / 提示见全局 src/styles/settings.css */
.streamer-page {
  display: flex;
  flex-direction: column;
}

.op-error {
  margin-bottom: 10px;
}

/* 数值带「元 / 行 / 秒」后缀，比全局默认宽一点，避免拖动时挤动滑块 */
.value {
  min-width: 62px;
}

.num-control {
  display: flex;
  align-items: center;
  gap: 8px;
}

.num {
  width: 76px;
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

.unit {
  color: var(--text-dim);
  font-size: 14px;
}

.error {
  color: var(--red);
  font-size: 15px;
  padding: 6px 0 0;
}

.saved {
  color: var(--green);
  font-size: 14px;
  padding-bottom: 10px;
}
</style>
