<script setup lang="ts">
import { defineProps, defineEmits } from 'vue'

const props = defineProps<{
  overlayVisible: boolean
  overlayClickthrough: boolean
  overlayAlwaysOnTop: boolean
  ttsEnabled: boolean
  giftTtsEnabled: boolean
  giftEnabled: boolean
  welcomeEnabled: boolean
  welcomeTtsGuard: boolean
  fontSize: number
}>()

const emit = defineEmits<{
  (e: 'update:overlayVisible', v: boolean): void
  (e: 'update:overlayClickthrough', v: boolean): void
  (e: 'update:overlayAlwaysOnTop', v: boolean): void
  (e: 'update:ttsEnabled', v: boolean): void
  (e: 'update:giftTtsEnabled', v: boolean): void
  (e: 'update:giftEnabled', v: boolean): void
  (e: 'update:welcomeEnabled', v: boolean): void
  (e: 'update:welcomeTtsGuard', v: boolean): void
  (e: 'update:fontSize', v: number): void
}>()

function toggleOverlayVisible() {
  emit('update:overlayVisible', !props.overlayVisible)
}

function toggleOverlayClickthrough() {
  emit('update:overlayClickthrough', !props.overlayClickthrough)
}

function toggleOverlayAlwaysOnTop() {
  emit('update:overlayAlwaysOnTop', !props.overlayAlwaysOnTop)
}

function toggleTtsEnabled() {
  emit('update:ttsEnabled', !props.ttsEnabled)
}

function toggleGiftTtsEnabled() {
  emit('update:giftTtsEnabled', !props.giftTtsEnabled)
}

function toggleGiftEnabled() {
  emit('update:giftEnabled', !props.giftEnabled)
}

function toggleWelcomeEnabled() {
  emit('update:welcomeEnabled', !props.welcomeEnabled)
}

function toggleWelcomeTtsGuard() {
  emit('update:welcomeTtsGuard', !props.welcomeTtsGuard)
}
</script>

<template>
  <div class="quick-control-card">
    <h4 class="card-title">快捷控制</h4>

    <!-- 弹幕窗开关 -->
    <div class="row-group">
      <div class="control-item">
        <span class="label">弹幕窗</span>
        <button class="btn-switch" :class="{ on: overlayVisible }" @click="toggleOverlayVisible">
          {{ overlayVisible ? '显示' : '隐藏' }}
        </button>
        <button class="btn-switch" :class="{ on: overlayClickthrough }" @click="toggleOverlayClickthrough">穿透</button>
        <button class="btn-switch" :class="{ on: overlayAlwaysOnTop }" @click="toggleOverlayAlwaysOnTop">置顶</button>
      </div>
    </div>

    <!-- 朗读开关 -->
    <div class="row-group">
      <div class="control-item">
        <span class="label">朗读</span>
        <button class="btn-switch" :class="{ on: ttsEnabled }" @click="toggleTtsEnabled">弹幕</button>
        <button class="btn-switch" :class="{ on: giftTtsEnabled }" @click="toggleGiftTtsEnabled">礼物</button>
      </div>
    </div>

    <!-- 礼物/欢迎开关 -->
    <div class="row-group">
      <div class="control-item">
        <span class="label">礼物/欢迎</span>
        <button class="btn-switch" :class="{ on: giftEnabled }" @click="toggleGiftEnabled">礼物</button>
        <button class="btn-switch" :class="{ on: welcomeEnabled }" @click="toggleWelcomeEnabled">欢迎</button>
        <button class="btn-switch" :class="{ on: welcomeTtsGuard }" @click="toggleWelcomeTtsGuard">舰长</button>
      </div>
    </div>

    <!-- 字号调节 -->
    <div class="row-group font-size-row">
      <span class="label">字号</span>
      <input
        type="range"
        v-model.number="fontSize"
        min="13"
        max="36"
        step="1"
        @input="$emit('update:fontSize', fontSize)"
        class="font-slider" />
      <span class="font-value">{{ fontSize }}px</span>
    </div>

    <p class="tip">开关立即生效</p>
  </div>
</template>

<style scoped>
.quick-control-card {
  background: var(--bg-side);
  border: 1px solid var(--border);
  border-radius: 8px;
  padding: 14px 16px;
  margin-top: 20px;
}

.card-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-dim);
  margin-bottom: 12px;
}

.row-group {
  margin-bottom: 14px;
}

.control-item {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.label {
  font-size: 15px;
  color: var(--text);
  font-weight: 500;
  min-width: 60px;
}

.btn-switch {
  font-size: 13px;
  padding: 6px 12px;
  border-radius: 5px;
  border: 1px solid var(--border);
  background: var(--bg-elev);
  color: var(--text);
  cursor: pointer;
  transition: all 0.15s;
}

.btn-switch:hover {
  background: var(--hover);
}

.btn-switch.on {
  background: var(--accent);
  color: var(--accent-strong-text);
  border-color: var(--accent);
}

.font-size-row {
  align-items: center;
  gap: 10px;
}

.font-slider {
  flex: 1;
  min-width: 120px;
  accent-color: var(--accent);
}

.font-value {
  font-size: 13px;
  color: var(--text-faint);
  min-width: 46px;
  text-align: right;
}

.tip {
  font-size: 12px;
  color: var(--text-faint);
  margin-top: 8px;
}
</style>
