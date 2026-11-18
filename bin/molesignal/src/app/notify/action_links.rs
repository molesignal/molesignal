// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Tokens of the one-time action links (see `domain::notify::action_links`): 32 random bytes in
//! URL-safe base64. Only their SHA-256 digest is stored.

use aws_lc_rs::{
    digest,
    rand::{SecureRandom, SystemRandom},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use kernel::{Error, Result};

/// A fresh token.
pub fn new_action_token() -> Result<String> {
    let mut bytes = [0_u8; 32];

    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| Error::internal("an action link token could not be generated"))?;

    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// The digest a token is stored under.
pub fn action_token_hash(token: &str) -> String {
    digest::digest(&digest::SHA256, token.trim().as_bytes())
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Where a link opens: the confirm page of the web app under MoleSignal's external address.
pub fn action_url(external_url: &str, token: &str) -> Option<String> {
    let base = external_url.trim().trim_end_matches('/');

    (!base.is_empty()).then(|| format!("{base}/notify/actions/{token}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_random_and_stored_as_digests() {
        let token = new_action_token().unwrap();

        assert_eq!(token.len(), 43);
        assert_ne!(token, new_action_token().unwrap());
        assert_eq!(action_token_hash(&token).len(), 64);
        assert_eq!(
            action_token_hash(&token),
            action_token_hash(&format!(" {token} "))
        );
    }

    #[test]
    fn a_link_needs_an_external_address() {
        assert_eq!(
            action_url("https://ms.example/", "t").as_deref(),
            Some("https://ms.example/notify/actions/t")
        );
        assert_eq!(action_url("  ", "t"), None);
    }
}
