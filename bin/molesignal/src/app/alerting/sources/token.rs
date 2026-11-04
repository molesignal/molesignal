// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 来源的 Token：与 API Token 同一个样子 `mspat-<36>`，生成与校验也用同一套：前 10 位用来找来源，
//! 后 26 位存 SHA-256 哈希。Token 只在新建和轮换时出现一次。

use crate::infra::persistence::repositories::api_tokens::{
    assemble_token, generate_token_parts, hash_secret, split_token, verify_secret,
};

/// 新发的 Token。
pub(super) struct IssuedToken {
    pub token: String,
    pub prefix: String,
    pub hash: String,
}

pub(super) fn issue() -> IssuedToken {
    let (prefix, secret) = generate_token_parts();

    IssuedToken {
        token: assemble_token(&prefix, &secret),
        hash: hash_secret(&secret),
        prefix,
    }
}

/// 请求带的 Token 拆成（前 10 位, 后 26 位）；样子不对返回 `None`。
pub(super) fn split(token: &str) -> Option<(&str, &str)> {
    split_token(token.trim())
}

/// 后 26 位和存下的哈希对得上。
pub(super) fn matches(secret: &str, hash: &str) -> bool {
    verify_secret(secret, hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_issued_token_is_found_by_its_prefix_and_checked_by_its_hash() {
        let issued = issue();
        let (prefix, secret) = split(&issued.token).unwrap();

        assert!(issued.token.starts_with("mspat-"));
        assert_eq!(prefix, issued.prefix);
        assert!(matches(secret, &issued.hash));
        assert!(!matches("0".repeat(26).as_str(), &issued.hash));
        assert!(split("msalert_whatever").is_none());
    }
}
