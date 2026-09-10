//! 时间与编码工具：DRM 令牌、JS 风格时间串、随机 hex
//!
//! 只放无状态的纯函数工具，与 Edge TTS 协议无关的逻辑不要加进来。

use super::TRUSTED_CLIENT_TOKEN;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Windows FILETIME 纪元（1601-01-01）相对 Unix 纪元的秒数
const WIN_EPOCH: i64 = 11_644_473_600;

/// DRM 令牌：时间戳切到 Windows FILETIME 纪元、向下取整到 5 分钟、转 100ns 单位后
/// 与固定客户端令牌拼接做 SHA256，取大写 hex
pub fn sec_ms_gec() -> String {
    let mut ticks = unix_secs() + WIN_EPOCH;
    ticks -= ticks % 300;
    let ticks = ticks * 10_000_000;
    let mut hasher = Sha256::new();
    hasher.update(format!("{ticks}{TRUSTED_CLIENT_TOKEN}").as_bytes());
    hex(&hasher.finalize(), true)
}

/// JavaScript 风格时间串（服务端照 Edge 的格式校验）：
/// `Sat Nov 15 2025 10:00:00 GMT+0000 (Coordinated Universal Time)`
pub fn date_string() -> String {
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let secs = unix_secs();
    let days = secs.div_euclid(86_400);
    let today = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{} {} {day:02} {year} {:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        WEEKDAYS[(days + 4).rem_euclid(7) as usize],
        MONTHS[(month - 1) as usize],
        today / 3600,
        today % 3600 / 60,
        today % 60,
    )
}

/// Unix 天数 → (年, 月, 日)，Howard Hinnant 的 civil_from_days 算法
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe as i64 + era * 400 + if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

fn unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 随机 hex 串（ConnectionId / RequestId / MUID，只需唯一性，不用于安全用途）
pub fn random_hex(bytes: usize, upper: bool) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut hasher = Sha256::new();
    hasher.update(nanos.to_le_bytes());
    hasher.update(COUNTER.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hex(&hasher.finalize()[..bytes], upper)
}

fn hex(bytes: &[u8], upper: bool) -> String {
    bytes
        .iter()
        .map(|b| {
            if upper {
                format!("{b:02X}")
            } else {
                format!("{b:02x}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec_ms_gec_is_upper_hex_sha256() {
        let token = sec_ms_gec();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(token.chars().all(|c| !c.is_ascii_lowercase()));
    }

    #[test]
    fn sec_ms_gec_rounds_down_to_five_minutes() {
        // 同一 5 分钟窗口内算两次必须一致（窗口切换瞬间可能不等，用两次快速取样降低概率）
        let first = sec_ms_gec();
        assert_eq!(first, sec_ms_gec());
    }

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn date_string_shape() {
        let s = date_string();
        assert!(s.ends_with(" GMT+0000 (Coordinated Universal Time)"));
        assert_eq!(s.len(), 62);
    }
}
