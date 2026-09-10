//! MP3 播放：直接调 winmm 的 MCI（`windows-sys` 已在依赖里，无需引入解码库）
//!
//! MCI 只认文件（不能吃内存缓冲），因此先把 MP3 落到临时文件再播；
//! Edge TTS 的输出固定是 MP3（服务端不支持 raw PCM，见 `Task/findings.md`）。
//!
//! 停止不跨线程调 MCI：由调用方传闭包进来，播放端每个轮询自己查（见 `play_mp3` 注释）。

use std::os::windows::ffi::OsStrExt;
use std::time::Duration;
use windows_sys::Win32::Media::Multimedia::{mciGetErrorStringW, mciSendStringW};

/// MCI 句柄别名：进程内唯一（队列串行播放，不会有两个句柄并发）
const ALIAS: &str = "bilidanmu_tts";
/// 轮询播放状态的间隔与上限（100ms × 600 = 60s）
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const POLL_MAX: u32 = 600;

/// 播放一段 MP3（阻塞至播完），失败返回可读原因
///
/// - `expired()` 开播前查一次：true 表示这条已被更新的弹幕顶掉，不响一声直接丢
/// - `stop_now()` 轮询里每次查：true 表示要立刻停（关掉朗读开关）
///
/// 「被打断」不掐正在念的这条：音频是整条合成的，掐断只能听到半句（开了念用户名时
/// 甚至连正文都没开始）。积压交给队列丢最旧 + `expired` 作废在途音频，
/// 延迟上限仍是一条朗读时长。
pub fn play_mp3(
    data: &[u8],
    expired: impl Fn() -> bool,
    stop_now: impl Fn() -> bool,
) -> Result<(), String> {
    // 从合成完毕到轮到播放之间可能已被顶掉（关了朗读 / 来了更新的弹幕）：连临时文件都不必落盘
    if expired() {
        return Ok(());
    }
    let path = std::env::temp_dir().join("bili-danmu-tts.mp3");
    std::fs::write(&path, data).map_err(|e| format!("写入临时音频失败: {e}"))?;

    // 上一轮异常退出可能残留句柄，先关掉再打开
    let _ = send(&format!("close {ALIAS}"));
    let result = (|| {
        send(&format!(
            "open \"{}\" type mpegvideo alias {ALIAS}",
            path.display()
        ))?;
        send(&format!("play {ALIAS}"))?;
        for _ in 0..POLL_MAX {
            std::thread::sleep(POLL_INTERVAL);
            if stop_now() {
                break;
            }
            // 查询失败（句柄已失效）按播放结束处理，避免死等
            if send(&format!("status {ALIAS} mode")).map_or(true, |m| m.trim() == "stopped") {
                break;
            }
        }
        Ok(())
    })();
    let _ = send(&format!("close {ALIAS}"));
    let _ = std::fs::remove_file(&path);
    result
}

/// 发一条 MCI 命令，返回其字符串结果（失败带 MCI 错误描述）
fn send(command: &str) -> Result<String, String> {
    let wide: Vec<u16> = std::ffi::OsStr::new(command)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut buf = [0u16; 256];
    let rc = unsafe {
        mciSendStringW(
            wide.as_ptr(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            std::ptr::null_mut(),
        )
    };
    if rc != 0 {
        return Err(format!("MCI 失败({rc}): {}", error_text(rc)));
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Ok(String::from_utf16_lossy(&buf[..len]))
}

fn error_text(code: u32) -> String {
    let mut buf = [0u16; 256];
    unsafe {
        mciGetErrorStringW(code, buf.as_mut_ptr(), buf.len() as u32);
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}
