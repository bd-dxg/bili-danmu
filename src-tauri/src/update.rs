//! 用系统默认程序打开外部链接 / 本地目录
//!
//! 版本检查、下载与安装已全部交给 `tauri-plugin-updater`：端点（国内加速优先、
//! GitHub 直连兜底）与公钥在 `tauri.conf.json` 的 `plugins.updater`，前端在
//! `composables/useUpdate.ts` 里调，本模块只负责「打开一个 http(s) 链接或目录」。
//!
//! 之所以还留着 `open_url` 而不是让前端直接开外链：链接由前端在系统浏览器打开，
//! 详见下方注释（wry 吞新窗口 + shell 会解析 `&`）。

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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    /// 主加速站上的更新清单（与 `tauri.conf.json` 的 `endpoints[0]` 同源）
    const MANIFEST_URL: &str = "https://gh-proxy.org/https://github.com/bd-dxg/bili-danmu/releases/latest/download/latest.json";

    /// 真实拉一次更新清单，并用**插件自己的解析器**解一遍（默认忽略）
    ///
    /// 断言的是发布流程里最容易写错的三件事：加速站能取到 latest.json、字段名对得上
    /// （`platforms` 的 `windows-x86_64` 键 + `url` / `signature`）、签名非空。
    /// 发布流程一改就跨过这一步，用户那边就是「检查更新失败」，所以留着这条联网回归。
    /// 注意：首个带 latest.json 的 Release 发布前必定 404，属预期。
    /// `cargo test --lib -- --ignored update_manifest_live --nocapture`
    #[test]
    #[ignore]
    fn update_manifest_live() {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .expect("构建客户端失败");
        let body = tauri::async_runtime::block_on(async {
            let resp = client.get(MANIFEST_URL).send().await.expect("请求端点失败");
            assert!(
                resp.status().is_success(),
                "端点返回 HTTP {}（清单还没上传？）",
                resp.status().as_u16()
            );
            resp.text().await.expect("读取响应失败")
        });
        let release: tauri_plugin_updater::RemoteRelease =
            serde_json::from_str(&body).expect("清单解析失败");
        let url = release
            .download_url("windows-x86_64")
            .expect("清单缺少 windows-x86_64 平台");
        let signature = release
            .signature("windows-x86_64")
            .expect("清单缺少签名");
        assert!(url.as_str().starts_with("https://"), "下载地址异常: {url}");
        assert!(!signature.trim().is_empty(), "签名为空");
        println!("version={} signature_len={}", release.version, signature.len());
        println!("url={url}");
    }
}
