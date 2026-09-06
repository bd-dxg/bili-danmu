# bili-danmu 开发指南

## 项目概览

轻量级 B 站直播弹幕桌面助手（Windows）：连接 B 站直播间 → 获取实时弹幕 → 在桌面透明悬浮层（Overlay）显示，供 OBS 推流主播看弹幕使用。Tauri 2 多窗口架构：主窗口（配置）+ 透明悬浮窗。

## 技术栈

- 前端：Vue 3（`<script setup>`）+ Vite + TypeScript，包管理 pnpm
- 桌面壳：Tauri 2（Rust，edition 2021）
- Rust 关键依赖：tokio / reqwest（json、gzip；登录 Cookie 全程显式 header 传递）/ tokio-tungstenite（弹幕 WebSocket）/ brotli-decompressor + flate2（弹幕包解压）/ md-5（WBI 签名）/ windows-sys（登录 Cookie 的 Windows DPAPI 加密）
- 授权：GPL-3.0

## 常用命令

- 安装依赖：`pnpm install`（根目录，会同时装 Rust 依赖需本机有 Rust 工具链）
- 开发运行：`pnpm tauri dev`（Vite 热更 + Rust 调试一起跑）
- 仅前端开发：`pnpm dev`
- 前端构建：`pnpm build`
- 打包桌面应用：`pnpm tauri build`
- 纯 Rust 编译检查：在 `src-tauri/` 下 `cargo check`
- PowerShell 辅助脚本：`scripts/`（`ui.ps1` 启动、`ws-probe.ps1` 协议探测、`gen-icon.ps1` 图标生成）

## 代码约定

- 目录结构：
  - `src/` 前端：`views/` 页面（RoomView 房间、DanmakuView 弹幕、AboutView 关于）、`overlay/OverlayApp.vue` 悬浮层入口、`composables/` 逻辑（如 useLogin）、`types/ipc.ts` IPC 类型定义、`overlay.ts` 独立入口（对应 `overlay.html`）
  - `src-tauri/src/bilibili/` Rust 端 B 站协议：`protocol.rs` 协议包、`parser.rs` 解压/解析、`client.rs` 连接、`wbi.rs` 签名、`login.rs` 扫码登录、`event.rs` 事件
  - `src-tauri/src/config.rs` 配置持久化、`lib.rs` 注册命令与窗口管理
- 多窗口：根目录 `index.html`（主窗口）+ `overlay.html`（透明悬浮窗），Tauri 配置见 `src-tauri/tauri.conf.json` 与 `capabilities/default.json`
- IPC 双向类型约定：Rust command 与 `src/types/ipc.ts` 保持一致，改动协议时两端同步
- 注释、commit、PRD（`prd.md`）一律简体中文
- PRD 即功能需求来源：已完成/规划状态以 `README.md` 表格和 `prd.md` 为准（TTS 朗读、滚动弹幕、屏蔽过滤均为未做项）

## 注意事项

- 禁止提交：`node_modules/`、`dist/`、`src-tauri/target/`、`src-tauri/gen/schemas/`、`Task/`、`dev.log`、根目录截图（`.gitignore` 已配）
- 弹幕为游客（uid=0）与登录两种链路，登录 Cookie 本地持久化；改动协议时用 `scripts/ws-probe.ps1` 验证
- 弹幕窗口性能敏感（120 条上限、透明层重绘），前端改动注意不要引入高频重排
- 登录 Cookie 在 `config.rs` 写盘边界统一 **DPAPI 加密**（`dpapi:` 前缀 + hex）落盘、读取自动解密（旧版明文无前缀兼容）：任何新增的敏感字段必须走同类加密，勿明文落盘；内存态保持明文
- `config.rs` 读写持有全局互斥（save_* 均为 load-modify-save）：新增保存函数须沿用 `lock_cfg()` + `*_unlocked` 模式，勿在持锁时调用加锁公共入口（std Mutex 不可重入）
- 断线自动重连在 `lib.rs` `spawn_connection`（1/2/4/…/30s 退避、10 次上限）；改连接收尾逻辑时保留取消令牌身份检查（`finish_connection`），否则旧任务会误清新连接状态
