// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! API 触发的令牌：只存 SHA-256 与前缀，令牌本身只在生成时出现一次；轮换后旧令牌立即失效。

use sha2::{Digest, Sha256};

use crate::infra::persistence::repositories::api_tokens::{generate_token, shown_prefix};

/// 新生成的令牌。
pub struct NewToken {
    pub token: String,
    pub hash: String,
    pub prefix: String,
}

pub fn generate() -> NewToken {
    let token = generate_token();

    NewToken {
        hash: hash(&token),
        prefix: shown_prefix(&token).to_string(),
        token,
    }
}

pub fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// 请求带的令牌和存下的哈希对得上；逐字节比较，不因前面对上的长短泄露时间。
pub fn matches(token: &str, stored_hash: &str) -> bool {
    let actual = hash(token);

    actual.len() == stored_hash.len()
        && actual
            .bytes()
            .zip(stored_hash.bytes())
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_random_shown_by_its_prefix_and_checked_by_its_hash() {
        let first = generate();
        let second = generate();

        assert!(first.token.starts_with("mspat-"));
        assert_ne!(first.token, second.token);
        assert!(first.token.starts_with(&first.prefix));
        assert!(first.prefix.len() < first.token.len());
        assert!(matches(&first.token, &first.hash));
        assert!(!matches(&second.token, &first.hash));
        assert!(!matches("", &first.hash));
    }
}
