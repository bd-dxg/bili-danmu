//! 本地配置读写（JSON，位于 Windows 应用数据目录，如 %APPDATA%\com.bilidanmu.app\config.json）
//!
//! 本模块只负责「读整份 → 改字段 → 写整份」的持久化：
//! - 结构体与默认值见 `types.rs`
//! - 登录 Cookie 的落盘加密见 `crypto.rs`

mod crypto;
mod types;

pub use types::*;

use crypto::{decrypt_cookie, encrypt_cookie};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

/// 配置读写互斥锁：save_* 均为「读整份 → 改字段 → 写整份」，并发交错会互相覆盖
/// （如拖动 Overlay 的 2s 落盘与设置样式同时保存）。
/// 加锁的公共入口调用 *_unlocked 内部实现，避免 std Mutex 嵌套死锁。
static CFG_LOCK: Mutex<()> = Mutex::new(());

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("获取配置目录失败: {e}"))?;
    Ok(dir.join("config.json"))
}

/// 加锁（互斥量中毒时恢复继续使用）
fn lock_cfg() -> std::sync::MutexGuard<'static, ()> {
    CFG_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 读取配置文件（不存在/损坏返回默认）
pub fn load_config(app: &AppHandle) -> ConfigFile {
    let _g = lock_cfg();
    load_config_unlocked(app)
}

fn load_config_unlocked(app: &AppHandle) -> ConfigFile {
    let path = match config_path(app) {
        Ok(p) => p,
        Err(_) => return ConfigFile::default(),
    };
    let mut cfg = match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => ConfigFile::default(),
    };
    // 登录 Cookie 在磁盘上为 DPAPI 加密串，读取后解密为内存明文（旧版无前缀明文直接兼容）；
    // 解密失败（更换系统账户/机器）时清除登录态，避免用坏 Cookie 反复请求
    if let Some(a) = cfg.auth.as_mut() {
        match decrypt_cookie(&a.cookies) {
            Ok(plain) => a.cookies = plain,
            Err(e) => {
                eprintln!(
                    "[config] 登录 Cookie 解密失败（可能更换了系统账户），已清除登录态: {e}"
                );
                cfg.auth = None;
            }
        }
    }
    cfg
}

/// 先写临时文件再改名落盘，避免中途崩溃留下损坏的配置文件
fn save_config_unlocked(app: &AppHandle, cfg: &ConfigFile) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    // 落盘前加密登录 Cookie：内存态保持明文，写盘边界统一加密，任何保存路径都不漏
    let mut out = cfg.clone();
    if let Some(a) = out.auth.as_mut() {
        a.cookies = encrypt_cookie(&a.cookies)?;
    }
    let content =
        serde_json::to_string_pretty(&out).map_err(|e| format!("序列化配置失败: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &content).map_err(|e| format!("写入临时配置失败: {e}"))?;
    match std::fs::rename(&tmp, &path) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Windows 下 rename 不覆盖已存在文件：删除旧配置后重试；
            // 仍失败（罕见）则回退直接写，保证配置能落盘
            let _ = std::fs::remove_file(&path);
            std::fs::rename(&tmp, &path)
                .or_else(|_| std::fs::write(&path, &content))
                .map_err(|e| format!("写入配置文件失败: {e}"))
        }
    }
}

/// 保存登录态
pub fn save_auth(app: &AppHandle, auth: &AuthInfo) -> Result<(), String> {
    let _g = lock_cfg();
    let mut cfg = load_config_unlocked(app);
    cfg.auth = Some(auth.clone());
    save_config_unlocked(app, &cfg)
}

/// 清除登录态
pub fn clear_auth(app: &AppHandle) -> Result<(), String> {
    let _g = lock_cfg();
    let mut cfg = load_config_unlocked(app);
    cfg.auth = None;
    save_config_unlocked(app, &cfg)
}

/// 保存 Overlay 样式
pub fn save_overlay_style(app: &AppHandle, style: &OverlayStyle) -> Result<(), String> {
    let _g = lock_cfg();
    let mut cfg = load_config_unlocked(app);
    cfg.overlay_style = style.clone();
    save_config_unlocked(app, &cfg)
}

/// 保存 Overlay 窗口位置大小
pub fn save_overlay_bounds(app: &AppHandle, bounds: &WindowBounds) -> Result<(), String> {
    let _g = lock_cfg();
    let mut cfg = load_config_unlocked(app);
    cfg.overlay_bounds = Some(*bounds);
    save_config_unlocked(app, &cfg)
}

/// 保存弹幕过滤配置
pub fn save_danmaku_filter(app: &AppHandle, filter: &DanmakuFilter) -> Result<(), String> {
    let _g = lock_cfg();
    let mut cfg = load_config_unlocked(app);
    cfg.danmaku_filter = filter.clone();
    save_config_unlocked(app, &cfg)
}

/// 保存 TTS 朗读配置
pub fn save_tts_config(app: &AppHandle, tts: &TtsConfig) -> Result<(), String> {
    let _g = lock_cfg();
    let mut cfg = load_config_unlocked(app);
    cfg.tts = tts.clone();
    save_config_unlocked(app, &cfg)
}

/// 保存礼物列表配置
pub fn save_gift_config(app: &AppHandle, gift: &GiftConfig) -> Result<(), String> {
    let _g = lock_cfg();
    let mut cfg = load_config_unlocked(app);
    cfg.gift = gift.clone();
    save_config_unlocked(app, &cfg)
}

/// 记录最近连接的直播间：同房间去重后置顶（顺带更新主播名），超出上限截断
pub fn save_recent_room(app: &AppHandle, room: &RecentRoom) -> Result<(), String> {
    let _g = lock_cfg();
    let mut cfg = load_config_unlocked(app);
    cfg.recent_rooms.retain(|r| r.room_id != room.room_id);
    cfg.recent_rooms.insert(0, room.clone());
    cfg.recent_rooms.truncate(RECENT_ROOM_LIMIT);
    save_config_unlocked(app, &cfg)
}
