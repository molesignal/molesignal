// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 登录和注册的存储：密码登录的次数、发信限额，和等邮箱验证的注册。

mod attempts;
#[cfg(test)]
mod memory;
mod rules;
mod signup_requests;

pub use attempts::{PgSigninAttemptRepository, SigninAttemptRepository};
#[cfg(test)]
pub(crate) use memory::MemoryAttempts;
pub use rules::{AttemptKeys, Begun, Quota, Refusal, Settlement, Tripped};
pub use signup_requests::{PgSignupRequestRepository, SignupRequestRepository};
