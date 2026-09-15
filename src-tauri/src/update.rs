//! 版本更新提醒：取 GitHub Releases 最新 tag 与本机版本比大小，有新版就提示
//!
//! 只做提醒、不做下载安装：一键更新要接 tauri-plugin-updater（minisign 密钥对、
//! latest.json 端点、capabilities 授权），发布流程也得跟着改，当前需求不需要。
//! 请求走 Rust 侧 reqwest 而不是前端 fetch：前端受 tauri.conf.json 的 CSP
//! connect-src 限制（目前只放开 ipc 与 dev 端口），放行 api.github.com 会扩大攻击面。

use std::time::Duration;
use tauri::AppHandle;

/// GitHub 最新 Release 接口（未认证限流 60 次/小时/IP；只在启动与手动点按时请求，够用）
const API_LATEST: &str = "https://api.github.com/repos/bd-dxg/bili-danmu/releases/latest";
/// 响应缺 html_url 时的兜底地址：发布列表页而非具体 tag，总能打开
const RELEASES_PAGE: &str = "https://github.com/bd-dxg/bili-danmu/releases";

#[derive(serde::Serialize)]
pub(crate) struct UpdateInfo {
    /// 最新版本（tag 去掉 v 前缀）
    pub(crate) latest: String,
    /// 最新版本是否比本机新
    pub(crate) has_update: bool,
    /// 新版本页面地址（交前端在浏览器打开）
    pub(crate) url: String,
}

/// 检查新版本（前端启动静默调一次 + 关于页手动重试）
#[tauri::command]
pub(crate) async fn check_update(app: AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    let client = build_client(&current)?;
    let (latest, url) = fetch_latest(&client).await?;
    Ok(UpdateInfo {
        has_update: is_newer(&current, &latest),
        latest,
        url,
    })
}

/// 构建更新检查用的 HTTP 客户端
fn build_client(ua_version: &str) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        // GitHub API 强制要求 User-Agent（缺失直接 403），按官方建议带应用名与版本
        .user_agent(format!("bili-danmu/{ua_version}"))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("构建 HTTP 客户端失败: {e}"))
}

/// 请求 GitHub 最新 Release，返回（版本主干，页面地址）
async fn fetch_latest(client: &reqwest::Client) -> Result<(String, String), String> {
    let resp = client
        .get(API_LATEST)
        .send()
        .await
        .map_err(|e| format!("请求 GitHub 失败: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        // 403 多为未认证限流，404 为仓库地址失效；网络不通则上面的 send 就失败了
        return Err(format!("GitHub 返回 HTTP {}", status.as_u16()));
    }
    let json = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("更新信息解析失败: {e}"))?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "更新信息缺少 tag_name".to_string())?;
    let url = json
        .get("html_url")
        .and_then(|v| v.as_str())
        .unwrap_or(RELEASES_PAGE)
        .to_string();
    Ok((normalize_tag(tag), url))
}

/// 用系统默认浏览器打开外部链接
///
/// 不走 `<a target="_blank">`：wry 默认吞掉 WebView2 的新窗口请求（见 AboutView 注释），
/// 点了没反应；也不走 `cmd /C start`：URL 要过一遍 shell 解析，`&` 会被当成命令分隔符。
/// ShellExecuteW 直接调 shell32，参数不经过命令行解析，还能指定窗口显示方式。
#[tauri::command]
pub(crate) fn open_url(url: String) -> Result<(), String> {
    // 只放行 http(s)：本命令虽只被自己人调用，但接口一旦存在就不该是任意协议
    // （file://、ms-settings: 等）的启动器
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("只允许打开 http(s) 链接".into());
    }
    open_in_shell(&url)
}

/// 用 shell 打开路径或链接（Windows 专属，与 player.rs 直接用 windows-sys 的做法一致）
///
/// 链接之外也供 `commands::open_log_dir` 打开本地目录：两者都走系统默认关联程序
/// （目录即资源管理器），不需要额外分支。
pub(crate) fn open_in_shell(url: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let op: Vec<u16> = "open\0".encode_utf16().collect();
    let file: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    let ret = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    // 返回值 ≤32 表示失败（SE_ERR_* 系列，是给 16 位兼容留的约定），成功返回 HINSTANCE
    if (ret as isize) <= 32 {
        return Err(format!("打开失败(code={})", ret as isize));
    }
    Ok(())
}

/// 取 tag 的版本主干（去掉可能存在的 `v` / `V` 前缀与首尾空白）
fn normalize_tag(tag: &str) -> String {
    tag.trim().trim_start_matches(['v', 'V']).to_string()
}

/// 版本号解析成三段数字
///
/// 不引 semver 依赖：发布 tag 固定为 `vX.Y.Z`，够用即可。每段取前导数字，
/// 故 `1.2.0-beta.1` 这类预发布 tag 只比前三段（`-beta` 段记 0）；缺段补 0。
fn parse_version(v: &str) -> (u64, u64, u64) {
    let norm = normalize_tag(v);
    let mut segs = norm.split('.').map(|seg| {
        seg.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u64>()
            .unwrap_or(0)
    });
    (
        segs.next().unwrap_or(0),
        segs.next().unwrap_or(0),
        segs.next().unwrap_or(0),
    )
}

/// `latest` 是否比 `current` 新
fn is_newer(current: &str, latest: &str) -> bool {
    parse_version(latest) > parse_version(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compare_by_number_not_string() {
        // 字符串比较会把 1.10.0 判成小于 1.9.0，必须是数字段比较
        assert!(is_newer("1.9.0", "1.10.0"));
        assert!(!is_newer("1.10.0", "1.9.0"));
    }

    #[test]
    fn compare_ignores_v_prefix() {
        assert!(is_newer("1.2.0", "v1.3.0"));
        assert!(!is_newer("v1.2.0", "1.2.0"));
        assert_eq!(normalize_tag(" v1.2.0 "), "1.2.0");
    }

    #[test]
    fn compare_prerelease_by_main_part() {
        // 预发布 tag 只比前三段：1.2.0-beta 不比 1.2.0 新
        assert!(!is_newer("1.2.0", "1.2.0-beta.1"));
        assert!(is_newer("1.2.0", "1.3.0-beta.1"));
    }

    #[test]
    fn compare_missing_segment_as_zero() {
        assert!(is_newer("1.2", "1.2.1"));
        assert!(!is_newer("1.2.0", "1.2"));
        assert!(is_newer("1.2.0", "2"));
    }

    /// 真实联网拉取最新 Release（默认忽略）：验证仓库地址仍有效、
    /// tag_name / html_url 的字段名与解析路径一致（GitHub 改响应结构时跑这个）
    /// `cargo test --lib -- --ignored update_fetches_latest_live --nocapture`
    #[test]
    #[ignore]
    fn update_fetches_latest_live() {
        let (latest, url) = tauri::async_runtime::block_on(async {
            let client = build_client("test").expect("构建客户端失败");
            fetch_latest(&client).await
        })
        .expect("拉取最新版本失败");
        println!("latest={latest} url={url}");
        assert!(latest.starts_with(char::is_numeric), "版本主干异常: {latest}");
        assert!(url.starts_with("https://github.com/"), "页面地址异常: {url}");
        // 与固定用例同源：解析结果必须能参与比较
        assert!(!is_newer(&latest, &latest));
    }
}
