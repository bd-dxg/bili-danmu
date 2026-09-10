//! 登录 Cookie 的落盘加密（Windows DPAPI）+ hex 编解码
//!
//! 落盘格式："dpapi:<hex>"。DPAPI 将数据绑定到当前 Windows 用户与机器，
//! 配置文件被拷贝到其它机器/账户后无法解密（读取时清除登录态重新扫码），
//! 但同机其他进程直接读文件拿不到明文 SESSDATA。

/// Cookie 加密串前缀（无此前缀视为加密功能引入前的旧版明文）
const COOKIE_ENC_PREFIX: &str = "dpapi:";

/// 加密登录 Cookie 供落盘（DPAPI → hex 编码 → 前缀标识）
pub(super) fn encrypt_cookie(plain: &str) -> Result<String, String> {
    let blob = dpapi_protect(plain.as_bytes())?;
    Ok(format!("{COOKIE_ENC_PREFIX}{}", hex_encode(&blob)))
}

/// 解密磁盘上存储的 Cookie：带前缀走 DPAPI 解密；无前缀视为旧版明文直接返回
pub(super) fn decrypt_cookie(stored: &str) -> Result<String, String> {
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
