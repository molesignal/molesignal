// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 多因素认证的规则：身份验证器 App 的验证码、恢复码、Passkey，输错多少次锁多久，等待验证的
//! 请求多久过期。存储在 [`crate::infra::mfa`]，接口在 `api/http/routes/mfa`。

pub mod passkey;
pub mod qr;
pub mod recovery;
pub mod totp;

use sha2::{Digest, Sha256};

use crate::infra::mfa::LockoutPolicy;

/// 已过第一关的登录等第二步验证的时间。
pub const SIGNIN_CHALLENGE_TTL_MICROS: i64 = 5 * 60 * 1_000_000;
/// 设置身份验证器时，等第一个验证码的时间。
pub const SETUP_CHALLENGE_TTL_MICROS: i64 = 10 * 60 * 1_000_000;
/// 等浏览器完成一次 Passkey 操作的时间，和 webauthn-rs 给浏览器的超时一样。
pub const PASSKEY_CHALLENGE_TTL_MICROS: i64 = 5 * 60 * 1_000_000;
/// 没有密码的账号加第二因素时，这次登录要在多久之内。
pub const RECENT_SIGN_IN_MICROS: i64 = 10 * 60 * 1_000_000;
/// 一个等待验证的请求输错几次作废。
pub const MAX_ATTEMPTS: i32 = 5;
/// 一个账号一小时内输错 10 次，锁一小时。
pub const LOCKOUT: LockoutPolicy = LockoutPolicy {
    window_micros: 60 * 60 * 1_000_000,
    limit: 10,
    lock_micros: 60 * 60 * 1_000_000,
};

/// 交给浏览器的请求凭据：32 字节随机数。
pub fn challenge_code() -> String {
    use base64::Engine as _;
    use rand::TryRng as _;

    let mut bytes = [0u8; 32];

    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .expect("operating-system random source");

    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// 库里存的请求凭据摘要。
pub fn challenge_digest(code: &str) -> String {
    hex::encode(Sha256::digest(code.trim().as_bytes()))
}

/// 身份验证器里显示的服务名：配了对外地址时带上主机名，分清几套实例。
pub fn issuer(external_url: &str) -> String {
    let host = url::Url::parse(external_url.trim())
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned));

    match host {
        Some(host) => format!("MoleSignal ({host})"),
        None => "MoleSignal".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_service_name_tells_instances_apart_by_host() {
        assert_eq!(
            issuer("https://ops.example.com/"),
            "MoleSignal (ops.example.com)"
        );
        assert_eq!(issuer(""), "MoleSignal");
    }

    #[test]
    fn a_challenge_code_is_random_and_stored_as_a_digest() {
        let code = challenge_code();

        assert_eq!(code.len(), 43);
        assert_ne!(code, challenge_code());
        assert_eq!(
            challenge_digest(&code),
            challenge_digest(&format!(" {code} "))
        );
        assert_eq!(challenge_digest(&code).len(), 64);
    }
}
