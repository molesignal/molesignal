// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 对象存储凭据解析：三层优先级 env > credentials_file > inline TOML。
//!
//! 启动期被 `bootstrap::build_state` 调用一次，emit `object_store_credentials_source` info log。

use crate::{
    config::ObjectStoreSettings,
    shared::{Error, Result},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialsSource {
    Env,
    File,
    Inline,
    None,
}

impl CredentialsSource {
    pub fn as_str(self) -> &'static str {
        match self {
            CredentialsSource::Env => "env",
            CredentialsSource::File => "file",
            CredentialsSource::Inline => "inline",
            CredentialsSource::None => "none",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedCredentials {
    pub access_key: String,
    pub secret_key: String,
    pub source: CredentialsSource,
}

impl ResolvedCredentials {
    pub fn empty() -> Self {
        Self {
            access_key: String::new(),
            secret_key: String::new(),
            source: CredentialsSource::None,
        }
    }

    pub fn is_set(&self) -> bool {
        !self.access_key.is_empty() && !self.secret_key.is_empty()
    }
}

pub fn resolve(cfg: &ObjectStoreSettings) -> Result<ResolvedCredentials> {
    // 1. env
    let env_key = std::env::var("MS_OBJECT_STORE_ACCESS_KEY").ok();
    let env_sec = std::env::var("MS_OBJECT_STORE_SECRET_KEY").ok();
    if let (Some(k), Some(s)) = (env_key, env_sec)
        && !k.is_empty()
        && !s.is_empty()
    {
        return Ok(ResolvedCredentials {
            access_key: k,
            secret_key: s,
            source: CredentialsSource::Env,
        });
    }
    // 2. credentials_file（首行 access_key=... 第二行 secret_key=...）
    if let Some(path) = &cfg.credentials_file {
        let text = std::fs::read_to_string(path)
            .map_err(|e| Error::internal(format!("read credentials_file {path:?}: {e}")))?;
        let mut access_key = String::new();
        let mut secret_key = String::new();
        for line in text.lines() {
            if let Some(v) = line.strip_prefix("access_key=") {
                access_key = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("secret_key=") {
                secret_key = v.trim().to_string();
            }
        }
        if !access_key.is_empty() && !secret_key.is_empty() {
            return Ok(ResolvedCredentials {
                access_key,
                secret_key,
                source: CredentialsSource::File,
            });
        }
    }
    // 3. inline TOML
    if !cfg.access_key.is_empty() && !cfg.secret_key.is_empty() {
        return Ok(ResolvedCredentials {
            access_key: cfg.access_key.clone(),
            secret_key: cfg.secret_key.clone(),
            source: CredentialsSource::Inline,
        });
    }
    Ok(ResolvedCredentials::empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_keys_resolve() {
        let cfg = ObjectStoreSettings {
            access_key: "ak".into(),
            secret_key: "sk".into(),
            ..Default::default()
        };
        let r = resolve(&cfg).unwrap();
        assert_eq!(r.source, CredentialsSource::Inline);
        assert_eq!(r.access_key, "ak");
    }

    #[test]
    fn empty_returns_none_source() {
        let cfg = ObjectStoreSettings::default();
        let r = resolve(&cfg).unwrap();
        assert_eq!(r.source, CredentialsSource::None);
        assert!(!r.is_set());
    }

    #[test]
    fn credentials_file_overrides_inline() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "access_key=from_file\nsecret_key=secret_file\n").unwrap();
        let cfg = ObjectStoreSettings {
            access_key: "inline_ak".into(),
            secret_key: "inline_sk".into(),
            credentials_file: Some(tmp.path().to_path_buf()),
            ..Default::default()
        };
        let r = resolve(&cfg).unwrap();
        assert_eq!(r.source, CredentialsSource::File);
        assert_eq!(r.access_key, "from_file");
    }
}
