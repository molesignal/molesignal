// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织的登录要求：一次登录能不能进入这个组织。
//!
//! 经本组织的 OIDC、OAuth 2.0、SAML 单点登录进入的会话两条要求都满足：多因素认证由身份提供方
//! 负责。LDAP 只核对目录里的密码，别的组织的单点登录也证明不了这个组织的事，都不算。
//!
//! 登录、切换组织、刷新会话时都按这里判断；普通请求不查，所以要求打开后，最迟一个 access token
//! 的有效期（15 分钟）对所有会话生效。

use super::SignInMethod;
use crate::{infra::mfa::SigninPolicy, shared::ids::Id};

/// 一次登录能不能进入一个组织。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Allowed,
    /// 还差多因素认证。
    NeedsMfa,
    /// 只能经这个组织的单点登录进入。
    NeedsSso,
}

/// 经 `method` 登录的会话能不能进入 `org_id`。`root` 不受「只允许单点登录」限制，身份提供方坏了
/// 还要有人能进来修；多因素认证的要求对 root 一样。
pub fn admission(
    policy: &SigninPolicy,
    method: &SignInMethod,
    org_id: &Id,
    root: bool,
) -> Admission {
    let own_sso = method.sso_org.as_ref() == Some(org_id);

    if policy.sso_only && !own_sso && !root {
        return Admission::NeedsSso;
    }

    if policy.require_mfa && !own_sso && !method.mfa {
        return Admission::NeedsMfa;
    }

    Admission::Allowed
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUIRE_MFA: SigninPolicy = SigninPolicy {
        require_mfa: true,
        sso_only: false,
    };
    const SSO_ONLY: SigninPolicy = SigninPolicy {
        require_mfa: false,
        sso_only: true,
    };

    fn org() -> Id {
        Id::from_string("org-1")
    }

    fn own_sso() -> SignInMethod {
        SignInMethod::single_sign_on(org())
    }

    fn other_sso() -> SignInMethod {
        SignInMethod::single_sign_on(Id::from_string("org-2"))
    }

    #[test]
    fn without_requirements_every_sign_in_enters() {
        let policy = SigninPolicy::default();

        for method in [
            SignInMethod::single_factor(),
            SignInMethod::second_factor(),
            SignInMethod::passkey(),
            own_sso(),
            other_sso(),
        ] {
            assert_eq!(
                admission(&policy, &method, &org(), false),
                Admission::Allowed
            );
        }
    }

    #[test]
    fn requiring_multi_factor_counts_a_second_factor_and_this_organizations_sso() {
        let enters = |method: &SignInMethod| admission(&REQUIRE_MFA, method, &org(), false);

        assert_eq!(enters(&SignInMethod::second_factor()), Admission::Allowed);
        assert_eq!(enters(&SignInMethod::passkey()), Admission::Allowed);
        assert_eq!(enters(&own_sso()), Admission::Allowed);
        assert_eq!(enters(&SignInMethod::single_factor()), Admission::NeedsMfa);
        assert_eq!(enters(&other_sso()), Admission::NeedsMfa);
    }

    #[test]
    fn single_sign_on_only_lets_in_this_organizations_sso() {
        let enters = |method: &SignInMethod| admission(&SSO_ONLY, method, &org(), false);

        assert_eq!(enters(&own_sso()), Admission::Allowed);
        assert_eq!(enters(&SignInMethod::single_factor()), Admission::NeedsSso);
        assert_eq!(enters(&SignInMethod::second_factor()), Admission::NeedsSso);
        assert_eq!(enters(&SignInMethod::passkey()), Admission::NeedsSso);
        assert_eq!(enters(&other_sso()), Admission::NeedsSso);
    }

    #[test]
    fn root_passes_single_sign_on_only_but_not_multi_factor() {
        assert_eq!(
            admission(&SSO_ONLY, &SignInMethod::single_factor(), &org(), true),
            Admission::Allowed
        );
        assert_eq!(
            admission(&REQUIRE_MFA, &SignInMethod::single_factor(), &org(), true),
            Admission::NeedsMfa
        );

        let both = SigninPolicy {
            require_mfa: true,
            sso_only: true,
        };

        assert_eq!(
            admission(&both, &SignInMethod::single_factor(), &org(), true),
            Admission::NeedsMfa
        );
        assert_eq!(
            admission(&both, &SignInMethod::second_factor(), &org(), true),
            Admission::Allowed
        );
        assert_eq!(
            admission(&both, &SignInMethod::second_factor(), &org(), false),
            Admission::NeedsSso
        );
        assert_eq!(
            admission(&both, &own_sso(), &org(), false),
            Admission::Allowed
        );
    }
}
