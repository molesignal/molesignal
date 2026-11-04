// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 多因素认证的接口。
//!
//! - 登录的第二步（`signin`）：第一关通过后存下待验证的登录，`POST /auth/signin/verify`
//!   带着验证码或 Passkey 换会话。
//! - 账号自己的设置（`account`）：`/me/mfa` 下开启、关闭身份验证器，换一组恢复码；
//!   `/me/sessions/revoke-others` 让别的设备退出登录。
//! - Passkey（`passkey`）：账号页添加、改名、删除，登录的第二步和免密码登录。
//! - 组织的登录要求（`requirements`）：`/settings/signin-policy`，以及登录、切换组织、刷新时的判断；
//!   要求多因素认证、账号还没有第二因素时，登录时现场开启（`enroll`）。
//! - 管理员（`admin`）：`POST /users/{id}/mfa/reset`。
//!
//! 规则和验证码算法在 [`crate::app::iam::mfa`]，存储在 [`crate::infra::mfa`]。

use axum::Router;

use crate::api::AppState;

mod account;
mod admin;
mod enroll;
mod factor;
mod notice;
mod passkey;
mod requirements;
mod signin;
mod totp_setup;

pub(crate) use enroll::begin as begin_enrollment;
pub(crate) use factor::{Checked, FactorProof, check as check_factor, rejection};
pub(crate) use requirements::{
    admission, ensure_keeps_sso_provider, ensure_still_admitted, sso_only,
};
pub(crate) use signin::{
    AfterVerification, after_first_step, enabled, finish_first_step, second_step,
    second_step_methods,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .merge(signin::routes())
        .merge(account::routes())
        .merge(passkey::routes())
        .merge(enroll::routes())
        .merge(requirements::routes())
        .merge(admin::routes())
}
