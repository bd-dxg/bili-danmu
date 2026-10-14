//! 时间与编码工具：DRM 令牌、JS 风格时间串、随机 hex
//!
//! 只放无状态的纯函数工具，与 Edge TTS 协议无关的逻辑不要加进来。

use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// (年, 月, 日) → Unix 天数，Howard Hinnant 的 days_from_civil 算法（`civil_from_days` 的逆）
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let m = month as i64;
    let y = if m <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    #[test]
    fn days_from_civil_inverts_civil_from_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2024, 1, 1), 19_723);
        for days in [-1, 1, 10_000, -10_000] {
            let (year, month, day) = civil_from_days(days);
            assert_eq!(days_from_civil(year, month, day), days);
        }
    }

    #[test]
    fn date_string_shape() {
        let s = date_string();
        assert!(s.ends_with(" GMT+0000 (Coordinated Universal Time)"));
        assert_eq!(s.len(), 62);
    }
}
