// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 身份验证器 App 的一次性验证码：RFC 6238 TOTP，HMAC-SHA1、6 位、30 秒一个时间步。
//!
//! Google Authenticator、Microsoft Authenticator、1Password 等 App 都按这套默认参数工作，
//! 所以不让人选算法和位数。

use aws_lc_rs::hmac;

/// 密钥长度：RFC 4226 建议的 160 位。
pub const SECRET_LEN: usize = 20;
/// 验证码位数。
pub const DIGITS: usize = 6;
/// 一个时间步的秒数。
pub const PERIOD_SECS: i64 = 30;
/// 前后各容几个时间步，抵消手机和服务器的时钟差。
const DRIFT_STEPS: i64 = 1;

/// 新的随机密钥。
pub fn generate_secret() -> Vec<u8> {
    use rand::TryRng as _;

    let mut secret = vec![0u8; SECRET_LEN];

    rand::rngs::SysRng
        .try_fill_bytes(&mut secret)
        .expect("operating-system random source");

    secret
}

/// `unix_secs` 所在的时间步。
pub fn step_at(unix_secs: i64) -> i64 {
    unix_secs.div_euclid(PERIOD_SECS)
}

/// 第 `step` 个时间步的验证码。
pub fn code_at(secret: &[u8], step: i64) -> String {
    truncated(secret, step, DIGITS)
}

fn truncated(secret: &[u8], step: i64, digits: usize) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let tag = hmac::sign(&key, &step.to_be_bytes());
    let hash = tag.as_ref();
    let offset = usize::from(hash[hash.len() - 1] & 0x0f);

    let binary = u32::from_be_bytes([
        hash[offset] & 0x7f,
        hash[offset + 1],
        hash[offset + 2],
        hash[offset + 3],
    ]);

    let modulus = 10u32.pow(digits as u32);

    format!("{:0digits$}", binary % modulus)
}

/// 在当前时间步前后找和 `code` 对上的时间步，只认比 `last_used_step` 晚的：同一个验证码
/// 不能用第二次。
pub fn matching_step(secret: &[u8], code: &str, now_secs: i64, last_used_step: i64) -> Option<i64> {
    let code = normalize(code)?;
    let current = step_at(now_secs);

    (current - DRIFT_STEPS..=current + DRIFT_STEPS)
        .filter(|step| *step > last_used_step)
        .find(|step| same(code_at(secret, *step).as_bytes(), code.as_bytes()))
}

/// 去掉空格，只认 6 位数字。
fn normalize(code: &str) -> Option<String> {
    let digits = code
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();

    (digits.len() == DIGITS && digits.bytes().all(|byte| byte.is_ascii_digit())).then_some(digits)
}

/// 逐字节比较，耗时与哪一位不同无关。
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

/// RFC 4648 base32：大写、不补 `=`，身份验证器 App 认这种写法。
pub fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

    let mut text = String::with_capacity(bytes.len().div_ceil(5) * 8);
    let mut buffer = 0u32;
    let mut bits = 0u32;

    for &byte in bytes {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;

        while bits >= 5 {
            bits -= 5;
            text.push(char::from(ALPHABET[((buffer >> bits) & 31) as usize]));
        }
    }

    if bits > 0 {
        text.push(char::from(ALPHABET[((buffer << (5 - bits)) & 31) as usize]));
    }

    text
}

/// 每 4 个字符空一格，方便手动输入。
pub fn grouped(text: &str) -> String {
    text.as_bytes()
        .chunks(4)
        .map(String::from_utf8_lossy)
        .collect::<Vec<_>>()
        .join(" ")
}

/// 身份验证器 App 扫码用的地址。`issuer` 是验证器里显示的服务名，`account` 是账号邮箱。
pub fn otpauth_url(issuer: &str, account: &str, secret: &[u8]) -> String {
    format!(
        "otpauth://totp/{label_issuer}:{account}?secret={secret}&issuer={issuer}&algorithm=SHA1&digits={DIGITS}&period={PERIOD_SECS}",
        label_issuer = percent_encode(issuer),
        account = percent_encode(account),
        secret = base32(secret),
        issuer = percent_encode(issuer),
    )
}

/// 只保留 RFC 3986 的非保留字符，其余都编码，各家 App 都能解开。
fn percent_encode(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());

    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }

    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC_SECRET: &[u8] = b"12345678901234567890";

    #[test]
    fn codes_match_the_rfc_6238_sha1_vectors() {
        for (time, expected) in [
            (59, "94287082"),
            (1_111_111_109, "07081804"),
            (1_111_111_111, "14050471"),
            (1_234_567_890, "89005924"),
            (2_000_000_000, "69279037"),
            (20_000_000_000, "65353130"),
        ] {
            assert_eq!(truncated(RFC_SECRET, step_at(time), 8), expected);
            assert_eq!(code_at(RFC_SECRET, step_at(time)), &expected[2..]);
        }
    }

    #[test]
    fn a_code_counts_one_step_either_side_and_only_once() {
        let now = 1_234_567_890;
        let current = step_at(now);

        assert_eq!(
            matching_step(RFC_SECRET, &code_at(RFC_SECRET, current), now, 0),
            Some(current)
        );
        assert_eq!(
            matching_step(RFC_SECRET, &code_at(RFC_SECRET, current - 1), now, 0),
            Some(current - 1)
        );
        assert_eq!(
            matching_step(RFC_SECRET, &code_at(RFC_SECRET, current - 2), now, 0),
            None
        );

        // Once a step is used, its code and the earlier ones no longer sign in.
        assert_eq!(
            matching_step(RFC_SECRET, &code_at(RFC_SECRET, current), now, current),
            None
        );
    }

    #[test]
    fn spaces_are_ignored_and_anything_but_six_digits_is_refused() {
        let now = 59;
        let code = code_at(RFC_SECRET, step_at(now));
        let spaced = format!("{} {}", &code[..3], &code[3..]);

        assert!(matching_step(RFC_SECRET, &spaced, now, 0).is_some());
        assert!(matching_step(RFC_SECRET, "12345", now, 0).is_none());
        assert!(matching_step(RFC_SECRET, "12345a", now, 0).is_none());
        assert!(matching_step(RFC_SECRET, "", now, 0).is_none());
    }

    #[test]
    fn base32_follows_rfc_4648_without_padding() {
        for (input, expected) in [
            ("", ""),
            ("f", "MY"),
            ("fo", "MZXQ"),
            ("foo", "MZXW6"),
            ("foob", "MZXW6YQ"),
            ("fooba", "MZXW6YTB"),
            ("foobar", "MZXW6YTBOI"),
        ] {
            assert_eq!(base32(input.as_bytes()), expected);
        }

        assert_eq!(base32(&generate_secret()).len(), 32);
        assert_eq!(grouped("ABCDEFGHIJ"), "ABCD EFGH IJ");
    }

    #[test]
    fn the_scan_address_names_the_service_and_the_account() {
        assert_eq!(
            otpauth_url(
                "MoleSignal (ops.example.com)",
                "alice@example.com",
                b"foobar"
            ),
            "otpauth://totp/MoleSignal%20%28ops.example.com%29:alice%40example.com?secret=MZXW6YTBOI&issuer=MoleSignal%20%28ops.example.com%29&algorithm=SHA1&digits=6&period=30"
        );
    }
}
