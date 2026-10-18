# 生成自动更新用的 latest.json（发布收尾）
#
# 完整发布流程（**必须用 pwsh 跑**，Windows PowerShell 5.1 会把无 BOM 的 UTF-8 中文注释读坏）：
#   $env:TAURI_SIGNING_PRIVATE_KEY = "$env:USERPROFILE\.tauri\bili-danmu.key"
#   $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""    # 空密码也要显式设！只设上面那个，
#                                                  # CLI 会弹密码提示并永久等输入（看起来像构建卡住）
#   pnpm tauri build
#   pwsh scripts/gen-latest-json.ps1 -Version 1.7.0
#   gh release create v1.7.0 src-tauri/target/release/bundle/nsis/bili-danmu_1.7.0_x64-setup.exe `
#     src-tauri/target/release/bundle/nsis/bili-danmu_1.7.0_x64-setup.exe.sig `
#     src-tauri/target/release/bundle/nsis/latest.json
#
# 依赖 tauri-cli >= 2.12.3：只有它会在签名里写 version 字段（由 tauri build 自动写入），
# 而客户端开着 requireSignedVersion 时会比对「清单声明的版本」与「签名绑定的版本」，
# 旧 CLI 签出的签名（没有 version）会被直接拒绝
#
# latest.json 是 tauri-plugin-updater 的端点内容：客户端按 tauri.conf.json 里 endpoints
# 的顺序依次试（加速站 → GitHub 直连），拿到清单后下载 url 指向的安装包，再用内置公钥
# 验签（签名即 .sig 文件内容）——验签不过就拒绝安装，加速站被投毒也装不上。
#
# 注意 url 只能写一个地址（清单里没有回退列表），所以这里写主加速站；加速站挂了走
# 关于页的「下载页」按钮手动下载。
param(
  [Parameter(Mandatory)][string]$Version,
  [string]$BundleDir = "src-tauri/target/release/bundle/nsis",
  [string]$Mirror = "https://gh-proxy.org",
  [string]$Out
)

$ErrorActionPreference = "Stop"

# 版本号统一按不带 v 的形式处理（tag 是 v1.7.0，配置里是 1.7.0）
$Version = $Version.TrimStart("v", "V")

# 版本号三处同步是发布流程的老坑，这里顺手挡一道：package.json 没改说明没升版本
$pkgVersion = (Get-Content "package.json" -Raw | ConvertFrom-Json).version
if ($pkgVersion -ne $Version) {
  throw "package.json 里是 $pkgVersion，与传入的 $Version 不一致——先同步 package.json / Cargo.toml / tauri.conf.json"
}

$exeName = "bili-danmu_${Version}_x64-setup.exe"
$exePath = Join-Path $BundleDir $exeName
$sigPath = "$exePath.sig"
if (-not (Test-Path $exePath)) { throw "找不到安装包：$exePath（先跑 pnpm tauri build）" }
if (-not (Test-Path $sigPath)) {
  throw "找不到签名文件：$sigPath（打包时没设 TAURI_SIGNING_PRIVATE_KEY，见本脚本头部）"
}
# 签名必须比安装包新：配置里漏了 createUpdaterArtifacts、或没带密钥时，tauri build 会
# 静默跳过签名，留下一个与安装包不匹配的旧 .sig——传上去就是「下载完验签失败」，
# 现象是用户那边永远装不上，而本地一切正常
if ((Get-Item $sigPath).LastWriteTimeUtc -lt (Get-Item $exePath).LastWriteTimeUtc) {
  throw "签名比安装包旧：删除 ` $sigPath ` 重新打包，并确认 tauri.conf.json 里 createUpdaterArtifacts 还在"
}

# 签名里绑定着版本号：requireSignedVersion 开着时客户端会比对「清单版本」与「签名版本」，
# 不一致或签名里没有 version，整批用户的更新都会失败——在生成清单这一步就挡住
$signature = (Get-Content $sigPath -Raw).Trim()
$signedVersion = [regex]::Match(
  [System.Text.Encoding]::UTF8.GetString([System.Convert]::FromBase64String(($signature -replace "\s", ""))),
  "version:([^\t\r\n]+)"
)
if (-not $signedVersion.Success) {
  throw "签名里没有 version 字段：确认打包用的是 tauri-cli >= 2.12.3（旧版签出的签名会被 requireSignedVersion 拒绝）"
}
if ($signedVersion.Groups[1].Value.Trim() -ne $Version) {
  throw "签名绑定的是 $($signedVersion.Groups[1].Value.Trim())，与清单的 $Version 不一致：客户端会拒绝安装"
}

# 安装包地址：加速站只替换域名，后面仍是 GitHub 的规范地址
$url = "$Mirror/https://github.com/bd-dxg/bili-danmu/releases/download/v$Version/$exeName"

$manifest = [ordered]@{
  version   = $Version
  pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = [ordered]@{
    "windows-x86_64" = [ordered]@{
      url       = $url
      signature = $signature
    }
  }
}

if (-not $Out) { $Out = Join-Path $BundleDir "latest.json" }
$json = $manifest | ConvertTo-Json -Depth 10
# UTF-8 无 BOM：带 BOM 的 JSON 有些解析器会直接报错
[System.IO.File]::WriteAllText($Out, $json, (New-Object System.Text.UTF8Encoding($false)))

Write-Host "已生成 $Out"
Write-Host "版本: $Version"
Write-Host "安装包: $url"
Write-Host ""
Write-Host "接下来把 exe / exe.sig / latest.json 三个文件一起传到 Release："
Write-Host "  gh release create v$Version --title v$Version --notes-file <说明文件> ``"
Write-Host "    '$exePath' ``"
Write-Host "    '$sigPath' ``"
Write-Host "    '$Out'"
