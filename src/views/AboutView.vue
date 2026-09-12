<script setup lang="ts">
import { getVersion } from '@tauri-apps/api/app'
import { onMounted, onUnmounted, ref } from 'vue'

import rewardCode from '../assets/reward-code.webp'

// 软件版本号：取自 Tauri 包信息（与 src-tauri/Cargo.toml / tauri.conf.json 同源），
// 不在前端再抄一份；浏览器里跑（pnpm dev 无 Tauri 注入）时拿不到，徽章自行隐藏
const version = ref('')

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

onMounted(async () => {
  try {
    version.value = await getVersion()
  } catch {
    // 拿不到版本号就不显示徽章，不影响页面其它内容
  }
})

onUnmounted(() => {
  if (tipTimer) clearTimeout(tipTimer)
})
</script>

<template>
  <div class="about-page">
    <!-- 「关于」包一层 span 是为了让版本徽章与文字之间只隔空白节点：
         纯空白节点（含换行）会被 Vue 丢掉，而文本节点里的换行会变成一格可见空格，
         把标题顶出一格。间距交给 .version 的 margin-left。 -->
    <h2>
      <span>关于</span>
      <span v-if="version" class="version">v{{ version }}</span>
    </h2>

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

    <div class="card support">
      <div class="support-text">
        <h3>支持项目</h3>
        <p>这个项目的功能都是业余时间开发和维护的：跟进 B 站协议改动、修 bug、加新功能。</p>
        <p>如果它帮你把直播弹幕看得更顺手，可以扫码支持一下，每份心意都是它继续更新的动力。</p>
        <p class="wx">微信扫码</p>
      </div>
      <img class="reward" :src="rewardCode" alt="微信赞赏码" />
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

/* 版本号徽章：与标题同行，不另占一行（关于页总高要压在内容区内） */
.version {
  margin-left: 6px;
  font-size: 13px;
  font-weight: 400;
  color: var(--text-faint);
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

/* 支持项目：文案在左、赞赏码在右。
   横向并排而不是上图下文：关于页总高要压在内容区以内（650 高的窗口减去标题栏
   与 content padding 约 580px），并排时卡片高度只由图决定（220 + 上下 padding）。 */
.support {
  display: flex;
  /* 垂直居中：图 220px 而文案只有三行，顶端对齐时左下角会空一块 */
  align-items: center;
  gap: 16px;
}

.support-text {
  flex: 1;
  min-width: 0;
}

.support p {
  font-size: 15px;
  line-height: 1.7;
  color: var(--text-dim);
}

/* 「微信扫码」单独成行而不是嵌在句子里：oxfmt 会把段落里与文字相间的行内元素
   拆到单独一行（无论多短都拆），拆出来的换行会被 Vue 渲染成一个可见空格，
   结果是「可以 微信扫码 支持一下」。单独占一行就没有这个问题。 */
.wx {
  margin-top: 4px;
  font-weight: 700;
  color: var(--green);
}

/* 赞赏码：图片自带白底，加圆角描边，深色主题下不会像一块浮白。
   这个尺寸（加上卡片 padding 共 240px）是看着页面总高定的：
   关于页 ≈537px，内容区 ≈578px，留约 40px 余量。 */
.reward {
  flex-shrink: 0;
  width: 220px;
  height: 220px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: #fff;
  object-fit: contain;
}
</style>
