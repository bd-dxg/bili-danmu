//! MP3 播放：直接调 winmm 的 MCI（`windows-sys` 已在依赖里，无需引入解码库）
//!
//! MCI 只认文件（不能吃内存缓冲），因此先把 MP3 落到临时文件再播；
//! Edge TTS 的输出固定是 MP3（服务端不支持 raw PCM，见 `Task/findings.md`）。

use std::os::windows::ffi::OsStrExt;
use std::time::Duration;
use windows_sys::Win32::Media::Multimedia::{mciGetErrorStringW, mciSendStringW};

/// MCI 设备别名（进程内唯一；队列串行播放，不会有两个句柄并发）
const ALIAS: &str = "bilidanmu_tts";
/// 轮询播放状态的间隔与上限（100ms × 600 = 60s）
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const POLL_MAX: u32 = 600;

/// 播放一段 MP3（阻塞至播完），失败返回可读原因
pub fn play_mp3(data: &[u8]) -> Result<(), String> {
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

/// 立即停止当前播放（关闭朗读开关时调用）
pub fn stop() {
    let _ = send(&format!("stop {ALIAS}"));
    let _ = send(&format!("close {ALIAS}"));
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
