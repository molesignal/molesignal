// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 多因素认证的存储：账号的第二因素（身份验证器、Passkey、恢复码）、输错锁定、等第二步验证的
//! 请求，和组织的登录要求。

mod challenges;
mod factors;
mod lockouts;
mod passkeys;
mod signin_policies;

pub use challenges::{
    MAX_PENDING_PER_USER, MfaChallenge, MfaChallengePurpose, MfaChallengeRepository,
    PgMfaChallengeRepository,
};
pub use factors::{
    MfaFactorRepository, NewTotp, PgMfaFactorRepository, TotpFactor, reset_by_email,
};
pub use lockouts::{Failure, LockoutPolicy, MfaLockoutRepository, PgMfaLockoutRepository};
pub use passkeys::{PasskeyRepository, PgPasskeyRepository, StoredPasskey, TurnOn};
pub use signin_policies::{PgSigninPolicyRepository, SigninPolicy, SigninPolicyRepository};
