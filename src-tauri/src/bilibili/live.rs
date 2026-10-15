//! 「已关注且正在开播」列表的数据源（主界面手动刷新）
//!
//! 两步：
//! ① `x/relation/followings` 拉关注列表（uid + 昵称），需登录 Cookie；
//! ② `room/v1/Room/get_status_info_by_uids` 批量查开播状态（匿名可用），
//!    只保留 `live_status == 1` 的 uid（未开播 / 轮播不算）。
//!
//! 实测结论（2026-10，探测脚本 `Task/probe-following.ps1`）：
//! - 关注列表带现有登录 Cookie + 浏览器化请求头即可，**无需 WBI 签名、无需 buvid3**；
//!   请求头 Referer 用 `space.bilibili.com`（关注接口所在站点）；
//! - 批量状态接口里**没有直播间的 uid 直接不返回 key**（不是报错），按 uid 缺失跳过；
//!   带空 Cookie 也能调通，故不传登录态。

use std::collections::HashMap;

/// 关注列表单页条数（接口上限 50）
const FOLLOWING_PAGE_SIZE: usize = 50;
/// 最多翻页数：500 人封顶，避免关注数上千时长时间串行请求
const FOLLOWING_MAX_PAGES: usize = 10;
/// 批量状态接口单次 uid 个数（分批防 URL 过长）
const STATUS_BATCH: usize = 100;

/// 一条「已关注且在播」记录
#[derive(Debug, Clone, serde::Serialize)]
pub struct FollowingLive {
    pub uid: i64,
    pub uname: String,
    /// 真实房间号（批量状态接口直接给真实号，点击可直连）
    pub room_id: u32,
}

/// 拉取关注列表（uid + 昵称），按关注时间倒序，最多 [`FOLLOWING_MAX_PAGES`] 页
pub async fn fetch_followings(
    client: &reqwest::Client,
    cookies: &str,
    uid: i64,
) -> Result<Vec<(i64, String)>, String> {
    let mut out: Vec<(i64, String)> = Vec::new();
    // 接口给的关注总数（用于判断是否翻完）与已收集条数（含昵称为空的）
    let mut total = 0i64;
    let mut seen = 0usize;
    for page in 1..=FOLLOWING_MAX_PAGES {
        let resp = client
            .get("https://api.bilibili.com/x/relation/followings")
            .query(&[
                ("vmid", uid.to_string()),
                ("pn", page.to_string()),
                ("ps", FOLLOWING_PAGE_SIZE.to_string()),
                ("order", "desc".into()),
                ("order_type", "attention".into()),
            ])
            .header("Cookie", cookies)
            .header("Referer", format!("https://space.bilibili.com/{uid}/fans/follow"))
            .send()
            .await
            .map_err(|e| format!("请求关注列表失败: {e}"))?
            .json::<serde_json::Value>()
            .await
            .map_err(|e| format!("关注列表响应解析失败: {e}"))?;

        let code = resp.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
        if code != 0 {
            return Err(format!(
                "获取关注列表失败(code={code}): {}",
                resp.get("message").and_then(|m| m.as_str()).unwrap_or("")
            ));
        }
        total = resp.pointer("/data/total").and_then(|v| v.as_i64()).unwrap_or(0);
        let list = resp
            .pointer("/data/list")
            .and_then(|l| l.as_array())
            .cloned()
            .unwrap_or_default();
        for item in &list {
            let Some(mid) = item.get("mid").and_then(|v| v.as_i64()) else {
                continue;
            };
            if mid <= 0 {
                continue;
            }
            seen += 1;
            let uname = item
                .get("uname")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if !uname.is_empty() {
                out.push((mid, uname));
            }
        }
        // 终止判定：以接口给的 total 为准。只看「本页不满」会在服务端把 ps 截断时
        // 误判为已到末页，静默漏掉后面几百人。
        if list.len() < FOLLOWING_PAGE_SIZE || (total > 0 && seen as i64 >= total) {
            break;
        }
    }
    // 关注数高于分页上限时只取前 N 人：列表仍可用，但日志里留痕（否则只能看到「在播很少」）
    if total > 0 && (seen as i64) < total {
        log::warn!(
            "[live] 关注数 {total} 超过分页上限（{} 人），仅检查前 {seen} 人",
            FOLLOWING_MAX_PAGES * FOLLOWING_PAGE_SIZE
        );
    }
    Ok(out)
}

/// 批量查询开播状态，返回「在播」的 uid → 真实房间号
///
/// 分批请求，**单批失败只跳过该批**（其余批次结果照常返回）：一次失败让整次刷新
/// 白跑对用户没意义；但全部批次都失败时返回错误，不能静默给出空列表
/// （空列表在 UI 上等于「没有关注在播」，与真失败会混淆）。
pub async fn fetch_live_rooms(
    client: &reqwest::Client,
    uids: &[i64],
) -> Result<HashMap<i64, u32>, String> {
    let batches: Vec<&[i64]> = uids.chunks(STATUS_BATCH).collect();
    let mut live = HashMap::new();
    let mut failed = 0usize;
    for chunk in &batches {
        match fetch_live_batch(client, chunk).await {
            Ok(batch) => live.extend(batch),
            Err(e) => {
                failed += 1;
                log::warn!("[live] 开播状态批次查询失败，已跳过该批: {e}");
            }
        }
    }
    if !batches.is_empty() && failed == batches.len() {
        return Err("开播状态查询失败（全部批次）".into());
    }
    Ok(live)
}

/// 查询一批 uid 的在播状态
async fn fetch_live_batch(
    client: &reqwest::Client,
    uids: &[i64],
) -> Result<HashMap<i64, u32>, String> {
    let query: Vec<(&str, String)> = uids.iter().map(|u| ("uids[]", u.to_string())).collect();
    let resp = client
        .get("https://api.live.bilibili.com/room/v1/Room/get_status_info_by_uids")
        .query(&query)
        .send()
        .await
        .map_err(|e| format!("请求开播状态失败: {e}"))?
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("开播状态响应解析失败: {e}"))?;

    let code = resp.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
    if code != 0 {
        return Err(format!(
            "获取开播状态失败(code={code}): {}",
            resp.get("message").and_then(|m| m.as_str()).unwrap_or("")
        ));
    }

    // 没有直播间的 uid 不会出现在 data 里（不是报错），按缺失跳过
    let mut live = HashMap::new();
    let Some(data) = resp.get("data").and_then(|d| d.as_object()) else {
        return Ok(live);
    };
    for (key, room) in data {
        if room.get("live_status").and_then(|v| v.as_i64()) != Some(1) {
            continue;
        }
        let Some(room_id) = room.get("room_id").and_then(|v| v.as_u64()) else {
            continue;
        };
        if let Ok(uid) = key.parse::<i64>() {
            live.insert(uid, room_id as u32);
        }
    }
    Ok(live)
}
