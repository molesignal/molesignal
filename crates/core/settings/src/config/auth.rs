// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[auth]` —— 登录会话时长 / root 账户引导。

use serde::{Deserialize, Serialize};

/// The JWT signing secret is not configured here: it lives in the `signing_secrets` table and is
/// created on first start. `MS_AUTH_JWT_SECRET_OVERRIDE` pins one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSettings {
    /// 一次登录持续的秒数：refresh token 的有效期。access token 在其中每 15 分钟换一次。
    #[serde(default = "default_session_ttl")]
    pub session_ttl_secs: u64,
    #[serde(default)]
    pub root_email: String,
    #[serde(default)]
    pub root_password: String,
}

fn default_session_ttl() -> u64 {
    86400
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            session_ttl_secs: default_session_ttl(),
            root_email: String::new(),
            root_password: String::new(),
        }
    }
}
