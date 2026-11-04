// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `[auth]` —— 登录会话时长 / root 账户引导与已弃用字段兼容。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSettings {
    // jwt_secret 字段已删除（auth-hardening）：JWT signing secret 由
    // `signing_secrets` 表持久化 + 首启动自动 bootstrap。
    // 锁定场景用 env `MS_AUTH_JWT_SECRET_OVERRIDE`（不在 Settings 里）。
    //
    // 旧配置（`[auth] jwt_secret = "..."`）会被 `#[serde(default)]` 容忍但忽略；
    // 升级提示打印在启动日志里。
    /// 兼容容器：吸收旧 `jwt_secret` 字段以便老配置不会拒绝解析（serde unknown_field）。
    /// 该字段读出后只用于打 deprecation 警告，**不参与签名**。
    #[serde(default, alias = "jwt_secret")]
    pub deprecated_jwt_secret: Option<String>,

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

impl AuthSettings {
    /// 启动期 deprecation check：若旧 jwt_secret 字段非空，打 warn 提示移除。
    pub fn warn_if_deprecated_secret_present(&self) {
        if self
            .deprecated_jwt_secret
            .as_deref()
            .is_some_and(|s| !s.is_empty())
        {
            tracing::warn!(
                "[auth].jwt_secret config field is deprecated (auth-hardening); \
                 remove it. JWT secret is now auto-bootstrapped to DB. \
                 To pin a specific secret, set MS_AUTH_JWT_SECRET_OVERRIDE env."
            );
        }
    }
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            deprecated_jwt_secret: None,
            session_ttl_secs: default_session_ttl(),
            root_email: String::new(),
            root_password: String::new(),
        }
    }
}
