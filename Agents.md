# bili-danmu 开发指南

## 项目概览

轻量级 B 站直播弹幕桌面助手（Windows）：连接 B 站直播间 → 获取实时弹幕 → 在桌面透明悬浮层（Overlay）显示 + 可选 Edge TTS 朗读，供 OBS 推流主播看/听弹幕。Tauri 2 多窗口架构：主窗口（配置）+ 透明悬浮窗（Overlay）+ 发送弹幕框（Sender，始终吸附 Overlay 下方）。

## 技术栈

- 前端：Vue 3（`<script setup>`）+ Vite + TypeScript，包管理 pnpm
- 桌面壳：Tauri 2（Rust，edition 2021）
- Rust 关键依赖：tokio / reqwest（json、gzip；登录 Cookie 全程显式 header 传递）/ tokio-tungstenite（弹幕 WebSocket + Edge TTS WebSocket）/ brotli-decompressor + flate2（弹幕包解压）/ md-5（WBI 签名）/ sha2（Edge TTS 的 Sec-MS-GEC 令牌）/ windows-sys（登录 Cookie 的 Windows DPAPI 加密、TTS 的 winmm MCI 播放）
- 授权：GPL-3.0

## 常用命令

- 安装依赖：`pnpm install`（根目录，会同时装 Rust 依赖需本机有 Rust 工具链）
- 开发运行：`pnpm tauri dev`（Vite 热更 + Rust 调试一起跑）
- 仅前端开发：`pnpm dev`
- 前端构建：`pnpm build`
- 打包桌面应用：`pnpm tauri build`
- 纯 Rust 编译检查：在 `src-tauri/` 下 `cargo check`
- 纯 Rust 单测：在 `src-tauri/` 下 `cargo test --lib`（含 `--ignored` 的联网测试：Edge TTS 合成与音色列表）
- PowerShell 辅助脚本：`scripts/`（`ui.ps1` 启动、`ws-probe.ps1` 弹幕协议探测、`edge-tts-probe.ps1` Edge TTS 协议与延迟/音色探测、`gen-icon.ps1` 图标生成）
  - 含中文的 `.ps1` 必须用 `pwsh` 跑（Windows PowerShell 5.1 会按 GBK 解码无 BOM 的 UTF-8 文件，中文被拆坏后连引号都会解析出错）

## 代码约定

- 目录结构：
  - `src/` 前端：`views/` 页面（RoomView 房间、DanmakuView 弹幕、TtsView 朗读、AboutView 关于，后三者均为页内标签式）、`components/` 复用组件（登录二维码弹窗、`DanmakuFilterPanel.vue` 弹幕筛选表单）、`overlay/OverlayApp.vue` 悬浮层入口、`sender/SenderApp.vue` 发送弹幕框入口、`composables/` 逻辑（如 useLogin）、`types/ipc.ts` IPC 类型定义、`overlay.ts` / `sender.ts` 独立入口（对应 `overlay.html` / `sender.html`）
  - `src-tauri/src/bilibili/` Rust 端 B 站协议：`protocol.rs` 协议包、`parser.rs` 解压/解析、`client.rs` 连接、`wbi.rs` 签名、`login.rs` 扫码登录、`send.rs` 发送弹幕、`event.rs` 事件
  - `src-tauri/src/tts/` Rust 端朗读：`edge.rs` Edge TTS 协议与音色列表、`player.rs` winmm MCI 播放、`mod.rs` 队列/流水线/打断/文案清洗
  - `src-tauri/src/config.rs` 配置持久化、`lib.rs` 注册命令与窗口管理
- 多窗口：根目录 `index.html`（主窗口）+ `overlay.html`（透明悬浮窗）+ `sender.html`（发送弹幕框），Tauri 配置见 `src-tauri/tauri.conf.json` 与 `capabilities/default.json`
- IPC 双向类型约定：Rust command 与 `src/types/ipc.ts` 保持一致，改动协议时两端同步
- 注释、commit、PRD（`prd.md`）一律简体中文
- PRD 即功能需求来源：已完成/规划状态以 `README.md` 表格和 `prd.md` 为准（滚动弹幕、用户屏蔽、Windows 系统 TTS、单实例、全局快捷键为未做项；弹幕过滤、发送弹幕、Edge TTS 朗读已完成）

## 注意事项

- 禁止提交：`node_modules/`、`dist/`、`src-tauri/target/`、`src-tauri/gen/schemas/`、`Task/`、`dev.log`、根目录截图（`.gitignore` 已配）
- 弹幕为游客（uid=0）与登录两种链路，登录 Cookie 本地持久化；改动协议时用 `scripts/ws-probe.ps1` 验证
- 弹幕窗口性能敏感（120 条上限、透明层重绘），前端改动注意不要引入高频重排
- 登录 Cookie 在 `config.rs` 写盘边界统一 **DPAPI 加密**（`dpapi:` 前缀 + hex）落盘、读取自动解密（旧版明文无前缀兼容）：任何新增的敏感字段必须走同类加密，勿明文落盘；内存态保持明文
- `config.rs` 读写持有全局互斥（save_* 均为 load-modify-save）：新增保存函数须沿用 `lock_cfg()` + `*_unlocked` 模式，勿在持锁时调用加锁公共入口（std Mutex 不可重入）
- 断线自动重连在 `lib.rs` `spawn_connection`（1/2/4/…/30s 退避、10 次上限）；改连接收尾逻辑时保留取消令牌身份检查（`finish_connection`），否则旧任务会误清新连接状态
- 发送弹幕需登录态（`bili_jct` 做 CSRF 签名，游客无 bili_jct 会报错）；发送框窗口始终吸附 Overlay 下方（`lib.rs` `sync_sender_docked`），缩进/宽度/高度随弹幕字号缩放——改动 Overlay 布局（`role-slot` 宽度、容器 padding）时须同步 `sender_layout_metrics` 的缩进系数，否则发送框与弹幕正文列错位
- 朗读与显示是**两套独立的 `DanmakuFilter`**（`danmaku_filter` / `tts.filter`）：显示筛选在前端 Overlay 做，朗读筛选在 Rust `on_danmaku` 做，改任一侧别把两者耦合成一份配置
- Edge TTS 协议集中在 `tts/edge.rs`：改协议用 `scripts/edge-tts-probe.ps1` 验证（`-DumpHeader` 看帧头、`-OutputFormat` 换格式），联网单测是 `cargo test --lib -- --ignored edge_synthesizes_mp3_live` / `voice_list_fetches_live`
- `tts/mod.rs` 的 `NAMED_CHARS`（`_`→下划线、`ω`→欧米伽等噪声字符表）是逐字实测得出的，加字符前先用探测脚本实测 Δ 字节数；实测数据在 `Task/findings.md`
- TTS 播放走 winmm MCI + 固定临时文件 `%TEMP%\bili-danmu-tts.mp3`，依赖队列串行（同一时刻只有一个 MCI 句柄）；`max_len` 只约束弹幕正文，不含身份前缀与用户名
