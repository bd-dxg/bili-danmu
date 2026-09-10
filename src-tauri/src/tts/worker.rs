//! 朗读流水线：合成任务 + 播放任务
//!
//! 合成与播放是两个任务：播第 N 条时已经在合成第 N+1 条，省掉每条的合成等待；
//! 播放严格串行，不会有多段语音重叠（prd.md §22）。

use super::{edge, TtsState};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;

/// 合成 → 播放通道容量：1 = 最多预合成 1 条
const PIPELINE_DEPTH: usize = 1;
/// 单条弹幕合成的总超时（含建连、发送、收流）
/// 12s：最长文本（`max_len = 0` 时 40 字）× 0.21s/字 + 握手 ~0.9s ≈ 9.3s，留余量即可；
/// 设太大（如 20s）时服务端黑洞会把合成任务卡到全队列被丢光才熔断
const SYNTH_TIMEOUT: Duration = Duration::from_secs(12);
/// 连续失败达到该次数即熔断退避：清空积压 + 指数退避后再试
const FAIL_THRESHOLD: u32 = 2;
/// 熔断退避起始时长（每多失败一次翻倍，封顶 FAIL_BACKOFF_MAX）
const FAIL_BACKOFF_MIN: Duration = Duration::from_secs(1);
const FAIL_BACKOFF_MAX: Duration = Duration::from_secs(60);

/// 已合成的音频；带 `epoch`，被新弹幕顶掉（打断）后作废
struct AudioChunk {
    data: Vec<u8>,
    epoch: u64,
    /// 试听：不受总开关限制，关着朗读也要能出声
    force: bool,
}

/// 启动朗读任务（应用启动时调用一次）：合成与播放各一个任务，之间用容量 1 的通道衔接
pub fn spawn_worker(app: AppHandle) {
    let (play_tx, mut play_rx) = mpsc::channel::<AudioChunk>(PIPELINE_DEPTH);

    // 播放任务：严格串行；代次过期的音频（已被更新弹幕顶掉 / 关了朗读）直接丢弃
    let player_app = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(chunk) = play_rx.recv().await {
            let epoch = chunk.epoch;
            let force = chunk.force;
            {
                let state = player_app.state::<TtsState>();
                if epoch != state.epoch.load(Ordering::Relaxed) {
                    continue;
                }
                state.playing.store(true, Ordering::Relaxed);
            }
            // MCI 播放要阻塞到播完，放阻塞线程池里跑，别占住异步 worker
            let play_app = player_app.clone();
            let data = chunk.data;
            let _ = tokio::task::spawn_blocking(move || {
                let expired =
                    || epoch != play_app.state::<TtsState>().epoch.load(Ordering::Relaxed);
                // 只有「关掉朗读」才立刻停下；被打断的只是还没开播的音频
                // （见 interrupt_if_backlogged），且试听不受总开关限制
                let stop_now = || !force && !play_app.state::<TtsState>().is_enabled();
                super::player::play_mp3(&data, expired, stop_now)
            })
            .await;
            player_app
                .state::<TtsState>()
                .playing
                .store(false, Ordering::Relaxed);
        }
    });

    // 合成任务：send 完立刻开始合成下一条，与播放重叠
    tauri::async_runtime::spawn(async move {
        // 连续失败计数：服务端拒绝（10054 / 黑洞超时）时指数退避。
        // 失败是秒回（~0.1s）、成功是两条 ~2s，若失败后立即重试下一条，
        // 请求速率会瞬时飙升到十倍以上、把服务端限流撞得更紧，且能自我维持——必须退避。
        let mut consecutive_fail = 0u32;
        loop {
            let item = app.state::<TtsState>().pop().await;
            let config = app.state::<TtsState>().config();
            // 关闭期间积压的弹幕在重新开启后不再补读（试听例外）
            if !config.enabled && !item.force {
                continue;
            }
            let epoch = app.state::<TtsState>().epoch.load(Ordering::Relaxed);
            // 总超时兜住建连阶段：edge::synthesize 内部只覆盖收流，网络黑洞时建连会拖到
            // 系统 TCP 超时（Windows ~21s），单合成任务被独占，期间队列持续丢弹幕
            let synth = tokio::time::timeout(
                SYNTH_TIMEOUT,
                edge::synthesize(&config.voice, config.rate_pct, config.volume_pct, &item.text),
            )
            .await;
            match synth {
                Ok(Ok(mp3)) => {
                    let state = app.state::<TtsState>();
                    // 合成期间可能已被关掉或被打断，丢弃过期产物，别出声音
                    let keep = (item.force || state.config().enabled)
                        && state.epoch.load(Ordering::Relaxed) == epoch;
                    drop(state);
                    if keep
                        && play_tx
                            .send(AudioChunk {
                                data: mp3,
                                epoch,
                                force: item.force,
                            })
                            .await
                            .is_err()
                    {
                        break; // 播放端已退出
                    }
                    consecutive_fail = 0;
                    continue;
                }
                // 单条失败只记录不重试：重试会让日志和请求雪崩（熔断见下）
                Ok(Err(e)) => eprintln!("[tts] 合成失败：{e}"),
                Err(_) => eprintln!("[tts] 合成超时（{SYNTH_TIMEOUT:?}）"),
            }

            // 熔断：积压的弹幕已经过期（听众关心的是新弹幕），先清掉；
            // 退避期间不发起任何请求，给服务端冷却时间，避开越撞越紧的恶性循环
            consecutive_fail += 1;
            if consecutive_fail < FAIL_THRESHOLD {
                continue;
            }
            // 指数封顶到 2^6（64s）再 min，否则持续失败几十分钟后 `2u32.pow` 会溢出 panic
            let shift = (consecutive_fail - FAIL_THRESHOLD).min(6);
            let backoff = (FAIL_BACKOFF_MIN * 2u32.pow(shift)).min(FAIL_BACKOFF_MAX);
            eprintln!("[tts] 连续失败 {consecutive_fail} 次，丢弃积压共暂停 {backoff:?} 后重试");
            app.state::<TtsState>().queue.lock().unwrap().clear();
            tokio::time::sleep(backoff).await;
        }
    });
}
