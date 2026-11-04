// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use sha2::{Digest, Sha256};

use crate::domain::iam::api_token::TOKEN_MARKER;

/// Lower case without `i l o u`: five random bits a character, and none to misread.
const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// The start of a token's random part. It is not secret: it finds the token's row.
const PREFIX_CHARS: usize = 10;

/// The rest of the random part, 130 bits.
const SECRET_CHARS: usize = 26;

pub fn generate_token_parts() -> (String, String) {
    use rand::TryRng as _;

    let mut bytes = [0_u8; PREFIX_CHARS + SECRET_CHARS];

    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .expect("operating system RNG");

    let value: String = bytes
        .iter()
        .map(|byte| ALPHABET[usize::from(*byte) % ALPHABET.len()] as char)
        .collect();

    let (prefix, secret) = value.split_at(PREFIX_CHARS);

    (prefix.to_string(), secret.to_string())
}

/// A new token for a credential that is found by the hash of its whole value, or by its owner.
pub fn generate_token() -> String {
    let (prefix, secret) = generate_token_parts();

    assemble_token(&prefix, &secret)
}

/// What may be shown of a token: its marker and the part of its random part that is not secret.
pub fn shown_prefix(token: &str) -> &str {
    token
        .get(..TOKEN_MARKER.len() + PREFIX_CHARS)
        .unwrap_or(token)
}

pub fn assemble_token(prefix: &str, secret: &str) -> String {
    format!("{TOKEN_MARKER}{prefix}{secret}")
}

pub fn split_token(token: &str) -> Option<(&str, &str)> {
    let random = token.strip_prefix(TOKEN_MARKER)?;

    if random.len() != PREFIX_CHARS + SECRET_CHARS {
        return None;
    }

    random.split_at_checked(PREFIX_CHARS)
}

/// A secret this long and random needs no slow password hash, and every request with a token
/// computes this.
pub fn hash_secret(secret: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(secret.as_bytes())))
}

pub fn verify_secret(secret: &str, hash: &str) -> bool {
    let Some(expected) = hash.strip_prefix("sha256:") else {
        return false;
    };

    constant_time_eq(
        expected.as_bytes(),
        hex::encode(Sha256::digest(secret.as_bytes())).as_bytes(),
    )
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_its_marker_and_one_run_of_unambiguous_characters() {
        let (prefix, secret) = generate_token_parts();
        let token = assemble_token(&prefix, &secret);
        let random = token.strip_prefix("mspat-").expect("the marker");

        assert_eq!(random.len(), 36);
        assert!(random.bytes().all(|byte| ALPHABET.contains(&byte)));
        assert_eq!(
            split_token(&token),
            Some((prefix.as_str(), secret.as_str()))
        );
        assert_eq!(shown_prefix(&token), format!("mspat-{prefix}"));
        assert!(split_token(&generate_token()).is_some());
    }

    #[test]
    fn only_a_token_of_the_exact_shape_splits() {
        assert!(split_token("mspat-tooshort").is_none());
        assert!(split_token(&format!("ms_{}", "a".repeat(36))).is_none());
        assert!(split_token(&format!("mspat-{}", "a".repeat(37))).is_none());

        // 36 bytes, with the place the prefix ends inside a character.
        assert!(split_token(&format!("mspat-{}é{}", "a".repeat(9), "a".repeat(25))).is_none());
    }

    #[test]
    fn the_hash_accepts_only_its_secret() {
        let hash = hash_secret("topsecret");

        assert!(verify_secret("topsecret", &hash));
        assert!(!verify_secret("wrong", &hash));
        assert!(!verify_secret("topsecret", "topsecret"));
    }
}
