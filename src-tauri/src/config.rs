//! 本地配置读写（JSON，位于 Windows 应用数据目录，如 %APPDATA%\com.bilidanmu.app\config.json）
//!
//! 字段随里程碑扩展：M2 登录 Cookie；M4 Overlay 弹幕样式。M6 扩展完整应用配置。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

/// B 站登录态（游客为空）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthInfo {
    /// 登录用户 UID（0 表示游客）
    pub uid: i64,
    /// 登录 Cookie 串（如 "SESSDATA=..; DedeUserID=.."）——内存保持明文供 HTTP 请求使用；
    /// 落盘时经 DPAPI 加密（见 encrypt_cookie），从磁盘读取后自动解密
    pub cookies: String,
    /// 用户昵称（nav 接口获取；旧配置无此字段为 None）
    #[serde(default)]
    pub uname: Option<String>,
}

/// Overlay 弹幕样式（M4）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayStyle {
    /// 弹幕字号（px）
    pub font_size: f64,
    /// 字体族
    pub font_family: String,
    /// 是否显示荣耀等级徽章
    pub show_wealth: bool,
    /// 是否显示粉丝牌
    pub show_medal: bool,
    /// 是否显示舰长/房管身份前缀
    pub show_role: bool,
    /// 用户名颜色（#RRGGBB）
    pub username_color: String,
    /// 弹幕内容颜色（#RRGGBB）
    pub content_color: String,
    /// 文字加粗
    pub bold: bool,
    /// 是否文字描边
    pub outline: bool,
    /// 描边颜色（#RRGGBB）
    pub outline_color: String,
    /// 描边宽度（px 近似值）
    pub outline_width: f64,
    /// 弹幕行间距（px，0=不额外加，仅行高）
    pub row_gap: f64,
}

impl Default for OverlayStyle {
    fn default() -> Self {
        Self {
            font_size: 17.0,
            font_family: "Microsoft YaHei UI".into(),
            show_wealth: true,
            show_medal: true,
            show_role: true,
            username_color: "#85DEF1".into(),
            content_color: "#FFFFFF".into(),
            bold: true,
            outline: true,
            outline_color: "#000000".into(),
            outline_width: 2.0,
            row_gap: 0.0,
        }
    }
}

/// Overlay 窗口位置大小（M3.5，逻辑坐标）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// 弹幕过滤配置
///
/// 身份规则间为「或」关系：任一开启的规则命中即显示（全部关闭 = 不过滤）；
/// 敏感词屏蔽独立叠加：开关开启且命中词表时整条丢弃，白名单规则不豁免。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DanmakuFilter {
    /// 只显示舰长（全部大航海）/ 房管弹幕
    pub enable_guard_admin: bool,
    /// 只显示有粉丝牌的弹幕（B 站实际只下发当前房间粉丝牌）
    pub enable_medal: bool,
    /// 只显示荣耀等级 ≥ wealth_min 的弹幕
    pub enable_wealth: bool,
    /// 荣耀等级门槛（配合 enable_wealth）
    pub wealth_min: u32,
    /// 屏蔽命中敏感词的弹幕（整条丢弃）
    pub enable_sensitive: bool,
    /// 敏感词表（弹幕内容含任一即丢弃）
    pub sensitive_words: Vec<String>,
}

impl Default for DanmakuFilter {
    fn default() -> Self {
        Self {
            enable_guard_admin: false,
            enable_medal: false,
            enable_wealth: false,
            wealth_min: 0,
            enable_sensitive: false,
            sensitive_words: Vec::new(),
        }
    }
}

/// TTS 弹幕朗读配置（M5）
///
/// 朗读筛选用独立的 `DanmakuFilter` 实例：显示全开、只朗读舰长这类组合才成立。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TtsConfig {
    /// 总开关
    pub enabled: bool,
    /// Edge TTS 音色名（如 zh-CN-XiaoxiaoNeural）
    pub voice: String,
    /// 语速百分比偏移（-50 = 半速，+50 = 1.5 倍速）
    pub rate_pct: i32,
    /// 音量百分比偏移
    pub volume_pct: i32,
    /// 是否在正文前念用户名
    pub read_username: bool,
    /// 是否在正文前念身份前缀（房管 / 舰长）
    pub read_role: bool,
    /// 弹幕正文最大朗读字数（不含身份前缀与用户名；超出截断，0 = 不限制）
    pub max_len: u32,
    /// 待朗读队列上限（超出丢弃最旧的）
    pub max_queue: u32,
    /// 积压时打断当前朗读（新弹幕直接顶掉正在念的那条，延迟上限压到一条朗读时长）
    pub interrupt_on_backlog: bool,
    /// 朗读筛选条件（与弹幕显示筛选相互独立）
    pub filter: DanmakuFilter,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            voice: "zh-CN-XiaoxiaoNeural".into(),
            rate_pct: 0,
            volume_pct: 0,
            read_username: false,
            read_role: false,
            // 15 字 ≈ 3.4s 音频；B 站弹幕上限 30–40 字（≈ 7–9s），全念完在高频房间会明显积压
            max_len: 15,
            max_queue: 5,
            interrupt_on_backlog: true,
            filter: DanmakuFilter::default(),
        }
    }
}

/// 最近连接过的直播间（主界面输入框下方面包屑，点击直连）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentRoom {
    /// 真实房间号（短号已由解析接口还原）
    pub room_id: u32,
    /// 主播昵称（接口获取失败为 None，前端回退显示房间号）
    #[serde(default)]
    pub uname: Option<String>,
}

/// 最近房间保留条数上限
pub const RECENT_ROOM_LIMIT: usize = 10;

/// 配置文件结构（后续里程碑扩展字段）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigFile {
    pub auth: Option<AuthInfo>,
    pub overlay_style: OverlayStyle,
    pub overlay_bounds: Option<WindowBounds>,
    pub danmaku_filter: DanmakuFilter,
    pub tts: TtsConfig,
    pub recent_rooms: Vec<RecentRoom>,
}

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

/// 记录最近连接的直播间：同房间去重后置顶（顺带更新主播名），超出上限截断
pub fn save_recent_room(app: &AppHandle, room: &RecentRoom) -> Result<(), String> {
    let _g = lock_cfg();
    let mut cfg = load_config_unlocked(app);
    cfg.recent_rooms.retain(|r| r.room_id != room.room_id);
    cfg.recent_rooms.insert(0, room.clone());
    cfg.recent_rooms.truncate(RECENT_ROOM_LIMIT);
    save_config_unlocked(app, &cfg)
}

// ---- 登录 Cookie 加密（Windows DPAPI）----
//
// 落盘格式："dpapi:<hex>"。DPAPI 将数据绑定到当前 Windows 用户与机器，
// 配置文件被拷贝到其它机器/账户后无法解密（读取时清除登录态重新扫码），
// 但同机其他进程直接读文件拿不到明文 SESSDATA。

/// Cookie 加密串前缀（无此前缀视为加密功能引入前的旧版明文）
const COOKIE_ENC_PREFIX: &str = "dpapi:";

/// 加密登录 Cookie 供落盘（DPAPI → hex 编码 → 前缀标识）
fn encrypt_cookie(plain: &str) -> Result<String, String> {
    let blob = dpapi_protect(plain.as_bytes())?;
    Ok(format!("{COOKIE_ENC_PREFIX}{}", hex_encode(&blob)))
}

/// 解密磁盘上存储的 Cookie：带前缀走 DPAPI 解密；无前缀视为旧版明文直接返回
fn decrypt_cookie(stored: &str) -> Result<String, String> {
    match stored.strip_prefix(COOKIE_ENC_PREFIX) {
        Some(hex) => {
            let blob = hex_decode(hex)?;
            let plain = dpapi_unprotect(&blob)?;
            String::from_utf8(plain).map_err(|e| format!("解密结果不是合法 UTF-8: {e}"))
        }
        None => Ok(stored.to_string()),
    }
}

/// hex 编码（小写）
fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((b & 0x0F) as u32, 16).unwrap());
    }
    out
}

/// hex 解码
fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("hex 串长度必须为偶数".into());
    }
    hex.as_bytes()
        .chunks(2)
        .map(|c| {
            let hi = (c[0] as char).to_digit(16).ok_or("hex 含非法字符")?;
            let lo = (c[1] as char).to_digit(16).ok_or("hex 含非法字符")?;
            Ok(((hi << 4) | lo) as u8)
        })
        .collect()
}

/// Windows DPAPI 加解密（CryptProtectData / CryptUnprotectData，数据绑定当前用户）
#[cfg(windows)]
mod dpapi {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    /// 加密（无 UI 提示，静默进行）
    pub fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
        let mut in_blob = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB::default();
        let ok = unsafe {
            CryptProtectData(
                &mut in_blob,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };
        if ok == 0 {
            return Err(format!(
                "CryptProtectData 失败: {}",
                std::io::Error::last_os_error()
            ));
        }
        let out = unsafe {
            std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec()
        };
        unsafe { LocalFree(out_blob.pbData as _) };
        Ok(out)
    }

    /// 解密
    pub fn unprotect(blob: &[u8]) -> Result<Vec<u8>, String> {
        let mut in_blob = CRYPT_INTEGER_BLOB {
            cbData: blob.len() as u32,
            pbData: blob.as_ptr() as *mut u8,
        };
        let mut out_blob = CRYPT_INTEGER_BLOB::default();
        let ok = unsafe {
            CryptUnprotectData(
                &mut in_blob,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };
        if ok == 0 {
            return Err(format!(
                "CryptUnprotectData 失败: {}",
                std::io::Error::last_os_error()
            ));
        }
        let out = unsafe {
            std::slice::from_raw_parts(out_blob.pbData, out_blob.cbData as usize).to_vec()
        };
        unsafe { LocalFree(out_blob.pbData as _) };
        Ok(out)
    }
}

#[cfg(windows)]
fn dpapi_protect(data: &[u8]) -> Result<Vec<u8>, String> {
    dpapi::protect(data)
}

#[cfg(windows)]
fn dpapi_unprotect(blob: &[u8]) -> Result<Vec<u8>, String> {
    dpapi::unprotect(blob)
}

/// 非 Windows 平台仅保证可编译（本项目面向 Windows）
#[cfg(not(windows))]
fn dpapi_protect(_data: &[u8]) -> Result<Vec<u8>, String> {
    Err("DPAPI 仅支持 Windows".into())
}

#[cfg(not(windows))]
fn dpapi_unprotect(_blob: &[u8]) -> Result<Vec<u8>, String> {
    Err("DPAPI 仅支持 Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_编解码往返() {
        let raw = b"\x00\x01\xab\xff\x10\x80";
        let enc = hex_encode(raw);
        assert_eq!(hex_decode(&enc).unwrap(), raw);
        assert!(hex_decode("abc").is_err(), "奇数长度应报错");
        assert!(hex_decode("zz").is_err(), "非法字符应报错");
    }

    #[cfg(windows)]
    #[test]
    fn dpapi_加解密往返() {
        let plain = "SESSDATA=abcdef123456; DedeUserID=10086";
        let cipher = dpapi_protect(plain.as_bytes()).expect("本机 DPAPI 加密应成功");
        let back = dpapi_unprotect(&cipher).expect("本机 DPAPI 解密应成功");
        assert_eq!(back, plain.as_bytes());
    }

    #[cfg(windows)]
    #[test]
    fn cookie_加密落盘与旧明文兼容() {
        let plain = "SESSDATA=xyz; DedeUserID=42";
        let stored = encrypt_cookie(plain).expect("加密应成功");
        assert!(stored.starts_with(COOKIE_ENC_PREFIX), "加密串应带前缀");
        assert!(!stored.contains(plain), "落盘内容不应包含明文");
        assert_eq!(decrypt_cookie(&stored).unwrap(), plain, "解密应还原明文");
        // 加密功能引入前的旧版明文配置：无前缀直接兼容可用
        assert_eq!(decrypt_cookie(plain).unwrap(), plain);
    }
}
