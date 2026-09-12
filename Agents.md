# bili-danmu 开发指南

## 项目概览

轻量级 B 站直播弹幕桌面助手（Windows）：连接 B 站直播间 → 获取实时弹幕 → 在桌面透明悬浮层（Overlay）显示 + 可选 Edge TTS 朗读，供 OBS 推流主播看/听弹幕。Tauri 2 多窗口架构：主窗口（配置）+ 透明悬浮窗（Overlay）+ 发送弹幕框（Sender，始终吸附 Overlay 下方）。

## 礼物系统（显示与朗读已完成，欢迎未开工）

打赏（礼物 / SC / 上舰）的**弹幕窗渲染**与**朗读**都已上线：解析 → 金额门槛 + 连击合并（`src-tauri/src/gift.rs`）→ 弹幕窗内独立礼物区（弹幕列表上方）；朗读侧另算一遍门槛与连击（`src-tauri/src/tts/gift.rs`）→ 插队送进 Edge TTS 队列。

- 解析：`parser.rs` 把 `SEND_GIFT` / `COMBO_SEND` / `SUPER_CHAT_MESSAGE` / `GUARD_BUY` 解成 `BilibiliEvent::Backing`；金额统一折算成整数「分」（`amount_fen`，金瓜子 1000 = 1 元），免费礼物（银瓜子 / 零价）直接返回 None
- `COMBO_SEND` **只当连击仍在继续的信号**（延长合并窗口），数量与金额一律以 `SEND_GIFT` 为准——两路事件同时下发，两边都计就重复；它是否带价格字段未实测（用 `scripts/ws-probe.ps1` 验证）
- 门槛与连击合并都在 Rust（`gift::on_backing`）：门槛按**累加后的总额**判定，未过门槛的连击先攒在分组里；同人同礼物在 `combo_window_secs` 内合并成一行，行 id = 分组键 + 首次时间戳，前端按 id **覆盖**更新（位置不变）
- 渲染：`GiftRow.vue` + `MetaBadges.vue`。徽章列抽成 `MetaBadges` 是**故意共用**的——`role-slot` 宽度与 `--role-indent` / 发送框左缩进（`window.rs` `sender_layout_metrics`）同源，复制一份 CSS 后改单侧就会错位；`row-style.ts` 同理（两行共用描边计算）。**礼物行不显示粉丝牌**：粉丝牌由弹幕行组装成 `medal` 对象传入，礼物行不传就不渲染（打赏行已有用户名与礼物名）；打赏事件（`Backing`）因此不再下发 `medal_*` 字段，前端也不再有 `DisplayBacking`，礼物行直接用 `BackingEvent`
- 设置项在「主播分区 → 礼物渲染」标签：开关 / 金额门槛 / 条数上限 / 合并窗口
- 礼物朗读（`tts/gift.rs` + `tts::on_backing`）：文案「感谢老板A送的5个辣条」/ SC 念留言正文（超 50 字截断）/ 上舰念档位；**插队**走 `TtsState::push_front`（排到待朗读弹幕之前），语义是「当前这条念完马上接」，**不**掐正在念的那条
- 礼物朗读**自己**按「用户 + 礼物」累加一遍，**不复用** `gift.rs` 的分组表：渲染侧只在过渲染门槛时才产出累计行，而朗读门槛与它独立（`GiftTtsConfig`，可更低）；连击静默满一个窗口才念一次，窗口沿用 `GiftConfig::combo_window_secs`（`tts::combo_window` 真接读 `OverlayState.gift`，改窗口只需改一处）；定时器是常驻轮询任务（`spawn_gift_timer`，1 秒一跳：按窗口整跳会让汇总最多晚一个窗口）
- 队列丢弃只丢**弹幕**（`tts/mod.rs` `push` 找最前的非 force 项）：插队的礼物在队首，直接 `pop_front` 会被下一条弹幕顶掉——高频直播间每秒几十条，上舰/醒目留言这种只来一次的大额插队后一条都活不到开播；熔断退避与总开关关闭同样只 retain 非 force 项
- 礼物朗读开关独立于弹幕朗读总开关与礼物区开关：进队时带 `force = true`（与试听同一语义），总开关关掉也照念；入队后按时念完；`set_config` 关闭总开关时只 retain 非 force 项（不偷删试听与礼物），`worker.rs` 的 epoch 作废与 `stop_now` 两道拦截也都跳过 force 项。设置项在「主播分区 → 礼物朗读」标签
- 未做：礼物图标（渲染层已留扩展位）、**欢迎信息**（进房观众 / 上舰 / 关注点赞）
- 待验证：`rnd` 去重未做——重连后服务端若重推同一条 `SEND_GIFT`，窗口内会重复累加，数量与金额翻倍

## 技术栈

- 前端：Vue 3（`<script setup>`）+ Vite + TypeScript，包管理 pnpm
- 桌面壳：Tauri 2（Rust，edition 2021）
- Rust 关键依赖：tokio / reqwest（json、gzip；登录 Cookie 全程显式 header 传递）/ tokio-tungstenite（弹幕 WebSocket + Edge TTS WebSocket）/ native-tls（Edge TTS 自建 TCP 后显式包 TLS）/ futures-util（WS 收发）/ brotli-decompressor + flate2（弹幕包解压）/ md-5（WBI 签名）/ sha2（Edge TTS 的 Sec-MS-GEC 令牌）/ windows-sys（登录 Cookie 的 Windows DPAPI 加密、TTS 的 winmm MCI 播放）
- 授权：GPL-3.0

## 常用命令

- 安装依赖：`pnpm install`（根目录，会同时装 Rust 依赖需本机有 Rust 工具链）
- 开发运行：`pnpm tauri dev`（Vite 热更 + Rust 调试一起跑）
- 仅前端开发：`pnpm dev`
- 前端构建：`pnpm build`
- 打包桌面应用：`pnpm tauri build`（产物在 `src-tauri/target/release/bundle/nsis/`）
- 纯 Rust 编译检查：在 `src-tauri/` 下 `cargo check`
- 纯 Rust 单测：在 `src-tauri/` 下 `cargo test --lib`（含 `--ignored` 的联网测试：Edge TTS 合成、音色列表、突发失败率）
- PowerShell 辅助脚本：`scripts/`（`ui.ps1` 启动、`ws-probe.ps1` 弹幕协议探测、`edge-tts-probe.ps1` Edge TTS 协议与延迟/音色探测、`gen-icon.ps1` 图标生成）
  - 含中文的 `.ps1` 必须用 `pwsh` 跑（Windows PowerShell 5.1 会按 GBK 解码无 BOM 的 UTF-8 文件，中文被拆坏后连引号都会解析出错）

## 代码约定

- 目录结构：
  - `src/` 前端：
    - `views/` 页面（RoomView 房间、DanmakuView 弹幕、TtsView 朗读、AboutView 关于，后三者均为页内标签式）
    - `components/` 复用组件（`LoginDialog.vue` 登录二维码弹窗、`DanmakuFilterPanel.vue` 弹幕筛选表单（显示/朗读共用）、`StatusDashboard.vue` 运行状态仪表盘、`SettingRow.vue` 设置行、`OverlayWindowPanel.vue` 弹幕窗标签页）
    - `composables/` 逻辑（`useLogin` 登录态、`useRoomConnection` 连接与最近房间、`useTtsConfig` 朗读配置与音色列表与校验、`useSaveTip` 设置页提示）
    - `overlay/OverlayApp.vue` + `overlay/DanmakuRow.vue` 悬浮层入口与单行渲染、`overlay/GiftRow.vue` 礼物行、`overlay/MetaBadges.vue` 两区共用的身份徽章列、`overlay/row-style.ts` 共用的描边计算、`sender/SenderApp.vue` 发送弹幕框入口
    - `styles/settings.css` 全局设置页样式（约束见「注意事项」）、`types/ipc.ts` IPC 类型定义、`overlay.ts` / `sender.ts` 独立入口（对应 `overlay.html` / `sender.html`）
  - `src-tauri/src/` Rust 核心：
    - `lib.rs` 组装（状态托管、窗口创建、托盘、命令注册）、`state.rs` 内存状态、`commands.rs` IPC 命令、`connection.rs` 连接与断线重连、`window.rs` 窗口辅助与发送框吸附
    - `bilibili/`：`protocol.rs` 协议包、`parser.rs` 解压/解析、`api.rs` HTTP 接口（房间解析、弹幕配置、主播名）、`ws.rs` WebSocket 会话、`wbi.rs` 签名、`login.rs` 扫码登录、`send.rs` 发送弹幕、`event.rs` 事件
    - `tts/`：`mod.rs` 队列状态与入口钩子（`on_danmaku` / `on_backing`）、`text.rs` 筛选/清洗/组装文案（弹幕与打赏各一个组装函数）、`gift.rs` 礼物朗读的连击聚合与静默窗口、`worker.rs` 合成与播放流水线、`player.rs` winmm MCI 播放、`edge.rs` Edge TTS 协议、`edge/voices.rs` 音色表、`edge/ssml.rs` SSML 构造、`edge/util.rs` 时间与编码工具
    - `config.rs` 配置读写与全局互斥、`config/types.rs` 结构体与默认值、`config/crypto.rs` DPAPI 加解密
    - `gift.rs` 礼物列表：金额门槛与连击合并（唯一判定处，前端只负责按 id 覆盖与条数上限）
- 多窗口：根目录 `index.html`（主窗口）+ `overlay.html`（透明悬浮窗）+ `sender.html`（发送弹幕框），Tauri 配置见 `src-tauri/tauri.conf.json` 与 `capabilities/default.json`
- IPC 双向类型约定：Rust command 与 `src/types/ipc.ts` 保持一致，改动协议时两端同步
- 注释、commit 一律简体中文；PRD（`prd.md`）已停止维护（见下）
- 功能状态与规划以 `README.md` 表格为准（未做项：礼物图标、欢迎信息、滚动弹幕、顶弹、用户屏蔽、Windows 系统 TTS、单实例、全局快捷键、开机自启、统一日志）；`prd.md` 仅作历史设计参考，新需求不要再往上写

## 版本号与发布

- 版本号要同步改 3 处：`package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`；`src-tauri/Cargo.lock` 由 `cargo check` / 构建自动更新
- 流程：改版本 → commit（`🚀 应用版本号升至 X.Y.Z（…）`）→ 推 `main` → `pnpm tauri build` → `gh release create`
- 安装包路径：`src-tauri/target/release/bundle/nsis/bili-danmu_<版本>_x64-setup.exe`
- 打包前确认应用没在运行，否则 NSIS 写 exe 会失败
- `gh release create` 的 tag 只建在远端，本地要 `git fetch --tags` 才看得到
- 仓库历史一律 squash 合并（main 上每个 PR 一个带 `(#N)` 的 commit）：`gh pr merge --squash` 时**别传 `--subject`**，否则 GitHub 不会自动追加 `(#N)`，破坏标题格式

## 注意事项

- 禁止提交：`node_modules/`、`dist/`、`src-tauri/target/`、`src-tauri/gen/schemas/`、`Task/`、`dev.log`、根目录截图（`.gitignore` 已配）
- 弹幕必须登录态：B 站 2025+ 不向游客推送 `DANMU_MSG`，游客级 token 只是登录路径失败时的兜底（能连上但收不到弹幕）。登录 Cookie 本地持久化；改动协议时用 `scripts/ws-probe.ps1` 验证
- 弹幕窗口性能敏感（120 条上限、透明层重绘），前端改动注意不要引入高频重排
- 登录 Cookie 在 `config.rs` 的 `save_config_unlocked`（唯一写盘边界）统一 **DPAPI 加密**（`dpapi:` 前缀 + hex）落盘，加解密在 `config/crypto.rs`，读取自动解密（旧版明文无前缀兼容）：任何新增的敏感字段必须走同类加密，勿明文落盘；内存态保持明文
- `config.rs` 读写持有全局互斥（save_* 均为 load-modify-save）：新增保存函数须沿用 `lock_cfg()` + `*_unlocked` 模式，勿在持锁时调用加锁公共入口（std Mutex 不可重入）
- 断线自动重连在 `connection.rs` `spawn_connection`（1/2/4/…/30s 退避、10 次上限）；改连接收尾逻辑时保留取消令牌身份检查（`finish_connection`），否则旧任务会误清新连接状态
- 发送弹幕需登录态（`bili_jct` 做 CSRF 签名，游客无 bili_jct 会报错）；发送框窗口始终吸附 Overlay 下方（`window.rs` `sync_sender_docked`），**左端与面板背景左边缘同一条线**（6px padding + `--panel-inset` 5.15em，不再对齐正文列）、**宽度取面板宽度的 80%**、高度随弹幕字号缩放——改 Overlay 布局（容器 padding、`--role-indent` / `--panel-inset`）时须同步 `sender_layout_metrics` 里的这几个系数
- 朗读与显示是**两套独立的 `DanmakuFilter`**（`danmaku_filter` / `tts.filter`）：显示筛选在前端 Overlay 做，朗读筛选在 Rust `tts::on_danmaku` 做，改任一侧别把两者耦合成一份配置
- `src/styles/settings.css` 是**全局样式**（无 scoped），类名是主窗口所有页面的共用契约：往里加规则前先确认不和已有页面的类名撞车（`StatusDashboard` 的 `.tip` 就因此改名 `.dashboard-tip`）；页面独有的差异项（如 `.select` 的 min-width）留在各自组件的 scoped 样式里覆盖
- `DisplayDanmaku`（弹幕事件 + `isRoomMedal` 派生标记）定义在 `types/ipc.ts`，Overlay 列表与 `DanmakuRow` 共用
- Edge TTS 协议集中在 `tts/edge.rs`（音色表在 `edge/voices.rs`、SSML 在 `edge/ssml.rs`）：改协议用 `scripts/edge-tts-probe.ps1` 验证（`-DumpHeader` 看帧头、`-OutputFormat` 换格式），联网单测是 `cargo test --lib -- --ignored edge_synthesizes_mp3_live` / `voice_list_fetches_live`
- `tts/text.rs` 的 `NAMED_CHARS`（`_`→下划线、`ω`→欧米伽等噪声字符表）是逐字实测得出的，加字符前先用探测脚本实测 Δ 字节数；实测数据在 `Task/findings.md`（该目录被 gitignore，仅本机可见）
- TTS 播放走 winmm MCI + 固定临时文件 `%TEMP%\bili-danmu-tts.mp3`，依赖队列串行（同一时刻只有一个 MCI 句柄）；`max_len` 只约束弹幕正文，不含身份前缀与用户名
- TTS 停播有两条互不相同的路径，别合并：①`epoch` 递增 = 作废**还没开播**的在途音频（积压时打断；正在念的那条念完，否则开了念用户名时只能听到半句名字）；②`worker.rs` 里 `player::play_mp3` 的 `stop_now` 闭包 = 每 100ms 查总开关，关掉朗读时立刻停。`force = true`（试听 / 礼物朗读）**两道拦截都免疫**：一旦入队就一定念完（礼物正在合成时弹幕到达不会把它作废）。不要改回「从其他线程直接调 MCI stop」：它会与 open/play 交错，且阻塞弹幕回调
- 礼物朗读仍受合成串行约束：“插队”只改变**队列顺序**（`push_front`），当前这条合成/播放完才能轮到打赏，故插队后仍有最多「一条弹幕的合成 + 播放」延迟；想提前只能掐断正在念的音频，那会听到半句（见上一条）
- 单条合成有两层超时：`tts/worker.rs` 的 `SYNTH_TIMEOUT`（调用方包住整条，含建连）与 `edge.rs` 的 `RECV_TIMEOUT`（收流阶段），别只留一个
- Edge TTS **不接受连接复用**：同一条连接发第二轮 speech.config + ssml 必被 RST（10054，实测）。`synthesize` 每条新建连接，别改成连接池/长连接
- Edge TTS 建连走 `edge.rs` 自研的 `dial_tcp`（自己解析 DNS、**IPv4 优先**、全部失败才回退 IPv6），别换回 `tokio_tungstenite::connect_async`：后者按 DNS 返回顺序连（Windows 上 IPv6 通常排前）且不会因链路劣化换地址族，国内 IPv6 直连微软偶发被 RST，现象是弹幕一路正常而朗读整段全挂（收流阶段 10054）。合成失败日志末尾带「对端 IP」就是为了区分是哪条路径出的问题
- 合成失败必须走熔断退避（`tts/worker.rs`：连续 2 次 → 清空积压 + 1/2/4…封顶 60s）：失败是 ~0.1s 级响应、成功是 ~2s，若“失败立即重试下一条”，请求速率会瞬时飙升十倍，把服务端限流撞得更紧并自我维持（表现为持续 10054 + 20s 黑洞超时）。排查用 `cargo test --lib -- --ignored edge_burst_live --nocapture`
