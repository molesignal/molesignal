// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 恢复码：丢了身份验证器时，每个恢复码可以代替验证码用一次。

use sha2::{Digest, Sha256};

use crate::shared::ids::Id;

/// 一组恢复码的个数。
pub const COUNT: usize = 10;
/// 每个恢复码的字符数，约 50 位熵。
const LEN: usize = 10;
/// 小写 base32 字母表：没有容易和字母看混的 0、1、8、9。
const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";

/// 新的一组恢复码，写成 `xxxxx-xxxxx`。
pub fn generate() -> Vec<String> {
    use rand::TryRng as _;

    (0..COUNT)
        .map(|_| {
            let mut bytes = [0u8; LEN];

            rand::rngs::SysRng
                .try_fill_bytes(&mut bytes)
                .expect("operating-system random source");

            let code = bytes
                .iter()
                .map(|byte| char::from(ALPHABET[usize::from(byte & 31)]))
                .collect::<String>();

            format!("{}-{}", &code[..LEN / 2], &code[LEN / 2..])
        })
        .collect()
}

/// 用户输入的恢复码：去掉空格和连字符，不分大小写。
pub fn normalize(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

/// 库里存的摘要。带上账号 id，一个账号的摘要不能拿去对照别的账号。
pub fn digest(user_id: &Id, normalized: &str) -> String {
    hex::encode(Sha256::digest(
        format!("{}:{normalized}", user_id.0).as_bytes(),
    ))
}

/// 给用户看的一组恢复码在库里存的摘要。
pub fn digests(user_id: &Id, codes: &[String]) -> Vec<String> {
    codes
        .iter()
        .map(|code| digest(user_id, &normalize(code)))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn a_set_holds_ten_distinct_codes_in_two_groups_of_five() {
        let codes = generate();

        assert_eq!(codes.len(), COUNT);
        assert_eq!(codes.iter().collect::<HashSet<_>>().len(), COUNT);

        for code in &codes {
            let (head, tail) = code.split_once('-').expect("two groups");
            assert_eq!(head.len(), 5);
            assert_eq!(tail.len(), 5);
            assert!(normalize(code).bytes().all(|byte| ALPHABET.contains(&byte)));
        }
    }

    #[test]
    fn typed_codes_are_compared_without_spaces_dashes_or_case() {
        assert_eq!(normalize(" ABCDE-fghij "), "abcdefghij");
        assert_eq!(normalize("abcde fghij"), "abcdefghij");
    }

    #[test]
    fn the_same_code_hashes_differently_for_another_account() {
        let first = digest(&Id::from_string("user-1"), "abcdefghij");

        assert_eq!(first.len(), 64);
        assert_ne!(first, digest(&Id::from_string("user-2"), "abcdefghij"));
        assert_eq!(first, digest(&Id::from_string("user-1"), "abcdefghij"));
    }

    #[test]
    fn stored_codes_are_digests_of_what_was_shown() {
        let user_id = Id::from_string("user-1");
        let codes = generate();
        let hashes = digests(&user_id, &codes);

        assert_eq!(hashes.len(), codes.len());
        assert_eq!(
            hashes[0],
            digest(&user_id, &normalize(&codes[0].to_uppercase()))
        );
    }
}
