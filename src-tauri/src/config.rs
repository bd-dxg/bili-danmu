//! 本地配置读写（JSON，位于 Windows 应用数据目录，如 %APPDATA%\com.bilidanmu.app\config.json）
//!
//! 字段随里程碑扩展：M2 登录 Cookie；M4 Overlay 弹幕样式。M6 扩展完整应用配置。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// B 站登录态（游客为空）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthInfo {
    /// 登录用户 UID（0 表示游客）
    pub uid: i64,
    /// 登录 Cookie 串（如 "SESSDATA=..; DedeUserID=.."）
    pub cookies: String,
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
}

impl Default for OverlayStyle {
    fn default() -> Self {
        Self {
            font_size: 17.0,
            font_family: "Microsoft YaHei UI".into(),
            show_wealth: true,
            show_medal: true,
            show_role: true,
            username_color: "#FFFFFF".into(),
            content_color: "#FFFFFF".into(),
            bold: false,
            outline: true,
            outline_color: "#000000".into(),
            outline_width: 2.0,
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

/// 配置文件结构（后续里程碑扩展字段）
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigFile {
    pub auth: Option<AuthInfo>,
    pub overlay_style: OverlayStyle,
    pub overlay_bounds: Option<WindowBounds>,
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("获取配置目录失败: {e}"))?;
    Ok(dir.join("config.json"))
}

/// 读取配置文件（不存在/损坏返回默认）
pub fn load_config(app: &AppHandle) -> ConfigFile {
    let path = match config_path(app) {
        Ok(p) => p,
        Err(_) => return ConfigFile::default(),
    };
    match std::fs::read_to_string(path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => ConfigFile::default(),
    }
}

/// 保存配置文件
pub fn save_config(app: &AppHandle, cfg: &ConfigFile) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let content =
        serde_json::to_string_pretty(cfg).map_err(|e| format!("序列化配置失败: {e}"))?;
    std::fs::write(&path, content).map_err(|e| format!("写入配置文件失败: {e}"))
}

/// 读取登录态
pub fn load_auth(app: &AppHandle) -> Option<AuthInfo> {
    load_config(app)
        .auth
        .filter(|a| !a.cookies.is_empty())
}

/// 保存登录态
pub fn save_auth(app: &AppHandle, auth: &AuthInfo) -> Result<(), String> {
    let mut cfg = load_config(app);
    cfg.auth = Some(auth.clone());
    save_config(app, &cfg)
}

/// 清除登录态
pub fn clear_auth(app: &AppHandle) -> Result<(), String> {
    let mut cfg = load_config(app);
    cfg.auth = None;
    save_config(app, &cfg)
}

/// 保存 Overlay 样式
pub fn save_overlay_style(app: &AppHandle, style: &OverlayStyle) -> Result<(), String> {
    let mut cfg = load_config(app);
    cfg.overlay_style = style.clone();
    save_config(app, &cfg)
}

/// 保存 Overlay 窗口位置大小
pub fn save_overlay_bounds(app: &AppHandle, bounds: &WindowBounds) -> Result<(), String> {
    let mut cfg = load_config(app);
    cfg.overlay_bounds = Some(*bounds);
    save_config(app, &cfg)
}

/// 读取 Overlay 窗口位置大小
pub fn load_overlay_bounds(app: &AppHandle) -> Option<WindowBounds> {
    load_config(app).overlay_bounds
}
