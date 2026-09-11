<script setup lang="ts">
import { ref } from "vue";

// 页内标签：gift / giftTts / welcome
const tab = ref<"gift" | "giftTts" | "welcome">("gift");
</script>

<template>
  <div class="streamer-page">
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
        <p class="tip">
          礼物 / SC / 上舰在弹幕窗的渲染设置（视觉区分、连击合并、免费礼物过滤、节流）。
          功能未实现。
        </p>
      </div>
    </section>

    <section v-show="tab === 'giftTts'" class="tab-pane">
      <div class="setting-card">
        <p class="tip">
          礼物 / SC / 上舰的朗读开关与文案规则（礼物名、数量、用户名、金额档位过滤）。
          功能未实现。
        </p>
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
</style>
