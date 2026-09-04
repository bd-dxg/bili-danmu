//! 本地配置读写（V1 最小实现：仅持久化 B 站登录 Cookie）
//!
//! 配置文件位于 Windows 应用数据目录（如 %APPDATA%\com.bilidanmu.app\config.json）
//! M6 将扩展为完整应用配置（窗口位置、字体、TTS 等）

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

/// 配置文件结构（后续里程碑扩展字段）
#[derive(Debug, Default, Serialize, Deserialize)]
struct ConfigFile {
    auth: Option<AuthInfo>,
}

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("获取配置目录失败: {e}"))?;
    Ok(dir.join("config.json"))
}

/// 从磁盘加载登录态
pub fn load_auth(app: &AppHandle) -> Option<AuthInfo> {
    let path = config_path(app).ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<ConfigFile>(&content)
        .ok()?
        .auth
        .filter(|a| !a.cookies.is_empty())
}

/// 保存登录态到磁盘
pub fn save_auth(app: &AppHandle, auth: &AuthInfo) -> Result<(), String> {
    let path = config_path(app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let content = serde_json::to_string_pretty(&ConfigFile {
        auth: Some(auth.clone()),
    })
    .map_err(|e| format!("序列化配置失败: {e}"))?;
    std::fs::write(&path, content).map_err(|e| format!("写入配置文件失败: {e}"))
}

/// 清除登录态
pub fn clear_auth(app: &AppHandle) -> Result<(), String> {
    let path = config_path(app)?;
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("删除配置文件失败: {e}"))?;
    }
    Ok(())
}
