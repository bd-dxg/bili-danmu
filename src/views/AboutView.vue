<script setup lang="ts"></script>

<template>
  <div class="about-page">
    <h2>关于</h2>

    <p class="intro">
      bili-danmu：轻量级 B 站直播弹幕助手。连接直播间，弹幕实时显示在桌面透明悬浮层，
      给用 OBS 推流的主播和想一起互动的水友一个干净、不挡画面的看弹幕工具。
    </p>

    <div class="card">
      <h3>致谢开源</h3>
      <ul class="thanks">
        <li>B 站弹幕协议相关接口的公开分析与实现</li>
        <li>
          <a href="https://github.com/SoraYjy/DanmuFree" target="_blank" rel="noreferrer">
            DanmuFree
          </a>
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
  font-size: 16px;
  margin-bottom: 6px;
}

.intro {
  font-size: 13px;
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
  font-size: 13px;
  color: var(--accent);
  margin-bottom: 8px;
}

.thanks {
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
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
  font-size: 13px;
  color: var(--text-dim);
  line-height: 1.7;
}
</style>
