<script setup lang="ts">
import { onUnmounted, ref } from 'vue'

// 仓库地址：页面展示短形式，复制到剪贴板的是带协议的规范 URL（浏览器能直接打开）
const REPO_LABEL = 'github.com/bd-dxg/bili-danmu'
const REPO_URL = `https://${REPO_LABEL}`

const copied = ref(false)
const copyError = ref('')
let tipTimer: ReturnType<typeof setTimeout> | undefined

// 不用 <a target="_blank">：Tauri 默认不处理 WebView2 的新窗口请求（wry 会直接
// SetHandled(true) 吞掉），链接点了没反应；剪贴板是纯 Web API，无需插件。
async function copyRepoUrl() {
  copyError.value = ''
  try {
    await navigator.clipboard.writeText(REPO_URL)
    copied.value = true
    if (tipTimer) clearTimeout(tipTimer)
    tipTimer = setTimeout(() => (copied.value = false), 1500)
  } catch {
    // 剪贴板被拒（罕见）：地址本身可选中，退化成手动复制
    copyError.value = '复制失败，请手动选中地址复制'
  }
}

onUnmounted(() => {
  if (tipTimer) clearTimeout(tipTimer)
})
</script>

<template>
  <div class="about-page">
    <h2>关于</h2>

    <p class="intro">
      bili-danmu：轻量级 B 站直播弹幕助手。连接直播间，弹幕实时显示在桌面透明悬浮层， 给用 OBS
      推流的主播和想一起互动的水友一个干净、不挡画面的看弹幕工具。
    </p>

    <div class="card">
      <h3>项目地址</h3>
      <div class="repo-row">
        <span class="repo-url">{{ REPO_LABEL }}</span>
        <button class="copy-btn" @click="copyRepoUrl()">
          {{ copied ? '✓ 已复制' : '复制' }}
        </button>
      </div>
      <p v-if="copyError" class="repo-error">{{ copyError }}</p>
    </div>

    <div class="card">
      <h3>致谢开源</h3>
      <ul class="thanks">
        <li>B 站弹幕协议相关接口的公开分析与实现</li>
        <li>
          <a href="https://github.com/SoraYjy/DanmuFree" target="_blank" rel="noreferrer">DanmuFree</a>
          （认证流程与 WBI 签名对齐的参考实现）
        </li>
      </ul>
    </div>

    <div class="card">
      <h3>技术栈</h3>
      <p class="stack">Tauri 2 + Vue 3 + TypeScript + Rust，面向 Windows。</p>
    </div>
  </div>
</template>

<style scoped>
.about-page {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

h2 {
  font-size: 18px;
  margin-bottom: 6px;
}

.intro {
  font-size: 15px;
  line-height: 1.8;
  color: var(--text-dim);
}

.card {
  background: var(--bg-side);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 10px 12px;
}

.card h3 {
  font-size: 15px;
  color: var(--accent);
  margin-bottom: 8px;
}

.thanks {
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 15px;
  line-height: 1.7;
}

.thanks a {
  color: var(--accent);
  text-decoration: none;
}

.thanks a:hover {
  text-decoration: underline;
}

.stack {
  font-size: 15px;
  color: var(--text-dim);
  line-height: 1.7;
}

.repo-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.repo-url {
  font-size: 15px;
  color: var(--text-dim);
  /* 全局禁了选中，这里放开：复制失败时还能手动选中 */
  user-select: text;
  overflow-wrap: anywhere;
}

.copy-btn {
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 5px 12px;
  font-size: 14px;
  color: var(--text-dim);
  background: none;
  cursor: pointer;
  white-space: nowrap;
}

.copy-btn:hover {
  color: var(--text);
  border-color: var(--accent);
}

.repo-error {
  font-size: 14px;
  color: var(--red);
  padding-top: 6px;
}
</style>
