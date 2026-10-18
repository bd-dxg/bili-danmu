import { invoke } from '@tauri-apps/api/core'
import { check, type Update } from '@tauri-apps/plugin-updater'
// 更新状态（模块级单例：App 启动静默查一次，关于页读同一份）
import { ref } from 'vue'

/** 新版本发布页：拿不到清单里的安装包地址时的兜底出口（国内多数网络打不开） */
const RELEASES_PAGE = 'https://github.com/bd-dxg/bili-danmu/releases/latest'

/**
 * 单次检查请求的超时
 *
 * 插件自己不给请求设超时，而端点全是第三方加速站：只收包不回包时 `check()` 会一直
 * 挂着，「检查中…」就永远转下去。超时按**每个端点**算，端点排了三个，全挂时
 * 约 3×10s 才给出失败。
 */
const CHECK_TIMEOUT_MS = 10_000

const checking = ref(false)
const installing = ref(false)
const progress = ref(0)
/** 远端最新版本号（空串 = 未知 / 没有新版） */
const latest = ref('')
/** 手动下载用的安装包地址（取自更新清单，带加速站前缀，浏览器可直接下） */
const downloadUrl = ref(RELEASES_PAGE)
const error = ref('')

// 插件返回的 Update 是类实例，**不能**塞进 ref：Vue 会包成响应式代理，
// downloadAndInstall 里的 this 就不是原对象了；也不需要响应式，当句柄用即可
let pending: Update | null = null

function message(e: unknown) {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  // JSON.stringify(undefined) 返回的是 undefined（不是字符串），直接返回会把 error.value 污染成 undefined
  return JSON.stringify(e) ?? String(e)
}

/**
 * 从更新清单里取安装包地址
 *
 * 取清单而不是拼固定地址：地址带加速站前缀（`https://gh-proxy.org/https://github.com/...`），
 * 国内浏览器能直接下；插件把服务端原始 JSON 留在 `rawJson`，两种清单形态（静态 platforms
 * / 动态顶层 url）都认，都没有则退回发布页。
 */
function pickDownloadUrl(update: Update | null): string {
  const raw = update?.rawJson as { url?: string; platforms?: Record<string, { url?: string }> } | undefined
  return raw?.platforms?.['windows-x86_64']?.url ?? raw?.url ?? RELEASES_PAGE
}

/**
 * 检查新版本
 *
 * @param silent 启动时的自动检查传 true：网络不通（含 GitHub 被墙、加速站挂掉）
 *               不该打扰用户；手动点按传 false，失败要能看到原因并可重试
 */
export async function checkUpdate(silent = false) {
  if (checking.value || installing.value) return
  checking.value = true
  try {
    const found = await check({ timeout: CHECK_TIMEOUT_MS })
    pending = found
    latest.value = found?.version ?? ''
    downloadUrl.value = pickDownloadUrl(found)
    error.value = ''
  } catch (e) {
    pending = null
    if (!silent) {
      error.value = message(e)
      latest.value = ''
    }
  } finally {
    checking.value = false
  }
}

/**
 * 下载并安装新版本
 *
 * Windows 上安装由 NSIS 静默完成（passive 模式带 `/R`）：插件把安装包交给安装器后
 * 直接 `process::exit(0)`，由安装器把新版本拉起来——所以成功路径实际上没有「之后」，
 * 也不要在前端调 relaunch。
 *
 * 但 installing 仍然要在 finally 里复位：上面那条依赖是跨层的隐式约定（插件改成不复位、
 * 安装器启动失败但没抛错、将来多一个 bundle 目标都会踩到），不复位的结果是按钮永久停在
 * 「安装中…」，而检查与重试都被 installing 挡着，用户只能重启程序。
 *
 * 下载不传 timeout：reqwest 的 timeout 是**整条请求**的总时长，慢网下 3MB 可能超过
 * 任何合理阈值，设了反而把正常下载判死；真卡死了用户重启程序即可重来。
 */
export async function installUpdate() {
  if (!pending || installing.value) return
  const update = pending
  installing.value = true
  progress.value = 0
  error.value = ''
  let downloaded = 0
  let total = 0
  try {
    await update.downloadAndInstall(event => {
      if (event.event === 'Started') {
        downloaded = 0
        total = event.data.contentLength ?? 0
        progress.value = 0
      } else if (event.event === 'Progress') {
        downloaded += event.data.chunkLength
        // 加速站没给 Content-Length 时停在 0%，只在拿得到总长时才推进百分比
        if (total > 0) progress.value = Math.min(99, Math.floor((downloaded / total) * 100))
      } else {
        progress.value = 100
      }
    })
  } catch (e) {
    error.value = message(e)
  } finally {
    // 成功路径正常走不到这里（进程已被 process::exit(0) 干掉），走到就说明进程没退
    installing.value = false
  }
}

/**
 * 在系统默认浏览器打开安装包地址（手动下载）
 *
 * 走加速站而不是 GitHub 发布页：能点的到「手动下载」的多半就是国内打不开 GitHub 的人。
 * WebView 内不能直接开外链，走 Rust 的 ShellExecuteW。
 */
export async function openManualDownload() {
  try {
    await invoke('open_url', { url: downloadUrl.value })
  } catch (e) {
    error.value = message(e)
  }
}

export function useUpdate() {
  return {
    checking,
    installing,
    progress,
    latest,
    error,
    checkUpdate,
    installUpdate,
    openManualDownload,
  }
}
