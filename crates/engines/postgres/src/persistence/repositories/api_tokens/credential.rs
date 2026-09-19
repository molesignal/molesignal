// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use sha2::{Digest, Sha256};

use crate::shared::{Error, Result};

pub fn generate_token_parts() -> (String, String) {
    use rand::TryRng as _;
    const ALPHA: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut bytes = [0_u8; 48];
    rand::rngs::SysRng
        .try_fill_bytes(&mut bytes)
        .expect("operating system RNG");
    let value: String = bytes
        .iter()
        .map(|byte| ALPHA[(*byte as usize) % ALPHA.len()] as char)
        .collect();
    (value[..16].to_string(), value[16..].to_string())
}

pub fn assemble_token(prefix: &str, secret: &str) -> String {
    format!("ms_{prefix}_{secret}")
}

pub fn assemble_rum_token(prefix: &str, secret: &str) -> String {
    format!("msrum_{prefix}_{secret}")
}

pub fn split_token(token: &str) -> Option<(&str, &str)> {
    split_with_prefix(token, "ms_")
}

pub fn split_rum_token(token: &str) -> Option<(&str, &str)> {
    split_with_prefix(token, "msrum_")
}

fn split_with_prefix<'a>(token: &'a str, marker: &str) -> Option<(&'a str, &'a str)> {
    let rest = token.strip_prefix(marker)?;
    let (prefix, secret) = rest.split_once('_')?;
    (prefix.len() == 16 && secret.len() == 32).then_some((prefix, secret))
}

pub fn hash_secret(secret: &str) -> Result<String> {
    use argon2::{
        Algorithm, Argon2, Params, Version,
        password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
    };
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default());
    argon2
        .hash_password(secret.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| Error::internal(format!("argon2 hash: {error}")))
}

pub fn verify_secret(secret: &str, hash: &str) -> bool {
    use argon2::{
        Argon2,
        password_hash::{PasswordHash, PasswordVerifier},
    };
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(secret.as_bytes(), &parsed)
        .is_ok()
}

pub fn hash_rum_secret(secret: &str) -> String {
    format!("sha256:{}", hex::encode(Sha256::digest(secret.as_bytes())))
}

pub fn verify_rum_secret(secret: &str, hash: &str) -> bool {
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
    fn token_formats_round_trip() {
        let prefix = "aB3kZ1xT9pQrU7nM";
        let secret = "dFgHjKl8eRvNcWxYz4tBmEqPaS2vG6Qz";
        assert_eq!(
            split_token(&assemble_token(prefix, secret)),
            Some((prefix, secret))
        );
        assert_eq!(
            split_rum_token(&assemble_rum_token(prefix, secret)),
            Some((prefix, secret))
        );
        assert!(split_rum_token(&assemble_token(prefix, secret)).is_none());
    }

    #[test]
    fn both_hash_strategies_reject_the_wrong_secret() {
        let argon = hash_secret("topsecret").expect("hash");
        assert!(verify_secret("topsecret", &argon));
        assert!(!verify_secret("wrong", &argon));
        let rum = hash_rum_secret("topsecret");
        assert!(verify_rum_secret("topsecret", &rum));
        assert!(!verify_rum_secret("wrong", &rum));
    }
}
