// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 放在地址里的 Token（值班日历订阅、心跳打卡）：只给出一次，库里只存 SHA-256。

use aws_lc_rs::digest;

pub use crate::infra::persistence::repositories::api_tokens::generate_token as new_token;

/// 存进库里的是 Token 的 SHA-256。
pub fn token_hash(token: &str) -> String {
    hex::encode(digest::digest(&digest::SHA256, token.as_bytes()).as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_random_and_only_its_hash_is_kept() {
        let one = new_token();
        let two = new_token();

        assert!(one.starts_with("mspat-"));
        assert_ne!(one, two);
        assert_eq!(token_hash(&one), token_hash(&one));
        assert_ne!(token_hash(&one), one);
    }
}
