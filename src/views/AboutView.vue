<script setup lang="ts">
import { getVersion } from '@tauri-apps/api/app'
import { invoke } from '@tauri-apps/api/core'
import { computed, onMounted, onUnmounted, ref } from 'vue'

import rewardCode from '../assets/reward-code.webp'
import { checkUpdate, installUpdate, openManualDownload, useUpdate } from '../composables/useUpdate'

// 软件版本号：取自 Tauri 包信息（与 src-tauri/Cargo.toml / tauri.conf.json 同源），
// 不在前端再抄一份；浏览器里跑（pnpm dev 无 Tauri 注入）时拿不到，徽章自行隐藏
const version = ref('')

// 仓库地址：页面展示短形式，复制到剪贴板的是带协议的规范 URL（浏览器能直接打开）
const REPO_LABEL = 'github.com/bd-dxg/bili-danmu'
const REPO_URL = `https://${REPO_LABEL}`

const copied = ref(false)
const copyError = ref('')
const logError = ref('')
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

// ---- 日志目录 ----
// 「朗读不出声 / 收不到弹幕」这类问题只能靠日志定位，按钮放在标题行（不另占一行，
// 关于页总高已接近内容区上限）；目录由 Rust 侧建好后用资源管理器打开。
async function openLogDir() {
  logError.value = ''
  try {
    await invoke('open_log_dir')
  } catch (e) {
    logError.value = String(e)
  }
}

// ---- 版本更新 ----
// 结果来自模块级单例（App 启动 5s 后已静默查过一次），这里只负责展示与手动重试
const { checking, installing, progress, latest, error: updateError } = useUpdate()

// 只有手动点过才显示「已是最新」：启动自动检查无结果时不该多一行无用文案
const checked = ref(false)

// 有远端版本号 = 有新版本（检查失败时会被清空，不会误报）
const hasUpdate = computed(() => Boolean(latest.value))

// 一个按钮走完「检查更新 → 检查中… → 发现新版本 → 下载 x% → 安装中… → 已是最新 /
// 重试」全部状态：不新增一行（关于页总高已接近内容区上限，见 .reward 注释）
const updateLabel = computed(() => {
  if (installing.value) return progress.value >= 100 ? '安装中…' : `下载 ${progress.value}%`
  if (checking.value) return '检查中…'
  if (updateError.value) return '重试'
  if (hasUpdate.value) return `发现新版本 v${latest.value}`
  return checked.value ? '已是最新' : '检查更新'
})

async function onUpdateClick() {
  if (installing.value) return
  // 有新版就先下载安装；下载失败后 latest 还在，再点就是重试下载
  if (hasUpdate.value) {
    await installUpdate()
    return
  }
  checked.value = true
  await checkUpdate()
}

// 按钮提示：装得怎么样、失败是为什么——都用 hover 提示，不占页面高度
const updateTitle = computed(() => {
  if (updateError.value) return updateError.value
  if (installing.value) return `正在下载 v${latest.value}`
  if (hasUpdate.value) return '点击下载并安装（装完自动重启）'
  return ''
})

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
      <!-- 更新提醒与版本徽章同行：关于页放不下额外一行（见 .reward 注释）。
           有新版时右侧多一个「手动下」：自动更新失败（加速站挂掉）时的出口，
           地址取自更新清单，自带加速站前缀 -->
      <button
        class="pill-btn"
        :class="{ 'has-update': hasUpdate && !installing }"
        :disabled="checking || installing"
        :title="updateTitle"
        @click="onUpdateClick()">
        {{ updateLabel }}
      </button>
      <button
        v-if="hasUpdate && !installing"
        class="pill-btn"
        title="用浏览器下载安装包（自动更新不可用时手动装）"
        @click="openManualDownload()">
        手动下
      </button>
      <!-- 日志按钮与更新徽章同行：排障时用户要能自己找到日志文件 -->
      <button
        class="pill-btn"
        :title="logError || '打开日志目录（反馈问题时请附上最新的 bili-danmu.log）'"
        @click="openLogDir()">
        日志
      </button>
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
      <p v-if="logError" class="repo-error">{{ logError }}</p>
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

/* 页头小胶囊（更新徽章 + 日志按钮）：与版本徽章同行，13px 不撑高 h2 ——
   关于页总高已接近内容区上限。更新按钮一个按钮承载「检查更新 / 检查中… /
   发现新版本 / 已是最新 / 重试」五态（见 updateLabel），避免再添一行提示文字。 */
.pill-btn {
  margin-left: 8px;
  padding: 2px 10px;
  font-size: 13px;
  font-weight: 400;
  color: var(--text-faint);
  background: none;
  border: 1px solid var(--border);
  border-radius: 999px;
}

.pill-btn:hover:not(:disabled) {
  color: var(--text);
  border-color: var(--accent);
}

.pill-btn:disabled {
  cursor: default;
}

/* 有新版：换成强调色胶囊，比灰色的「检查更新」显眼 */
.pill-btn.has-update {
  color: var(--accent-strong-text);
  background: var(--accent-soft);
  border-color: var(--accent);
}
</style>
