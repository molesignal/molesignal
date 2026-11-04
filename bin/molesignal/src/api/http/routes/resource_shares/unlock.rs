// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 公开分享链接的访问密码。
//!
//! 输错的次数和登录共用 [`SigninLimiter`]：一个链接算一个账号，锁只落在发起请求的来源上，链接本身
//! 只限速，别人输错密码不会把看过这个链接的人挡在外面。规则在 [`crate::app::iam::signin_limit`]。

use std::net::IpAddr;

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::json;

use super::{
    audit::{ShareVisitor, record_audit},
    public_share_context,
};
use crate::{
    api::{AppState, http::signin_limit::SigninSource},
    app::iam::{
        check_password,
        signin_limit::{SigninAccount, SigninError, SigninLimiter},
    },
    shared::{Error, Result},
};

#[derive(Debug, Deserialize)]
pub(super) struct UnlockRequest {
    password: String,
}

pub(super) async fn public_unlock(
    State(state): State<AppState>,
    SigninSource(source): SigninSource,
    visitor: ShareVisitor,
    headers: HeaderMap,
    Json(request): Json<UnlockRequest>,
) -> Result<Response, SigninError> {
    let (session, share) = public_share_context(&state, &headers, false).await?;

    let Some(password_hash) = share.password_hash.as_deref() else {
        return Ok(unlocked());
    };

    let account = SigninAccount::ResourceShare {
        share_id: &share.id,
        org_id: &share.organization_id,
    };

    let checked = check_access_password(
        &state.iam.signin_limiter,
        &account,
        &request.password,
        password_hash,
        source,
    )
    .await;

    if let Err(SigninError::Failed(Error::Unauthorized(_))) = checked {
        record_audit(
            &state,
            &share,
            "resource_share.password_failure",
            "share_session",
            &session.id.0,
            Some(&visitor),
            json!({"result": "denied"}),
        )
        .await?;

        return Ok(wrong_password());
    }

    checked?;

    state
        .storage
        .resource_shares
        .mark_password_verified(&session.id)
        .await?;

    record_audit(
        &state,
        &share,
        "resource_share.unlock",
        "share_session",
        &session.id.0,
        Some(&visitor),
        json!({"result": "allowed"}),
    )
    .await?;

    Ok(unlocked())
}

fn unlocked() -> Response {
    Json(json!({"unlocked": true})).into_response()
}

/// 密码不对。会话过期也是 401，靠 `code` 才分得清，页面不会把它们都说成密码错。
fn wrong_password() -> Response {
    let body = json!({
        "error": "unauthorized",
        "code": "share_password_invalid",
        "message": "invalid share password",
    });

    (StatusCode::UNAUTHORIZED, Json(body)).into_response()
}

/// 核对访问密码。到限时拒绝，不核对；密码不对回答 [`Error::Unauthorized`]。首尾的空白不算密码，创建
/// 时就去掉了，复制时多带的空格不该算一次输错。
async fn check_access_password(
    limiter: &SigninLimiter,
    account: &SigninAccount<'_>,
    password: &str,
    password_hash: &str,
    source: Option<IpAddr>,
) -> Result<(), SigninError> {
    let attempt = limiter.begin(account, source).await?;
    let outcome = check_password(password.trim(), password_hash).await;

    attempt.settle(&outcome).await;

    Ok(outcome?)
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use argon2::{
        Algorithm, Argon2, Params, Version,
        password_hash::{PasswordHasher, SaltString, rand_core::OsRng},
    };
    use axum::body::to_bytes;

    use super::*;
    use crate::{
        app::iam::{signin_limit::SigninRefused, test_support::signin_limiter},
        domain::iam::SigninLimits,
        shared::ids::Id,
    };

    const RIGHT: &str = "open sesame";
    const WRONG: &str = "wrong password";

    /// 核对时用哈希里写的参数，所以用最便宜的参数就不用等几十次真的 Argon2。
    static HASH: LazyLock<String> = LazyLock::new(|| {
        Argon2::new(
            Algorithm::Argon2id,
            Version::V0x13,
            Params::new(8, 1, 1, None).unwrap(),
        )
        .hash_password(RIGHT.as_bytes(), &SaltString::generate(&mut OsRng))
        .unwrap()
        .to_string()
    });

    fn from(last: u8) -> Option<IpAddr> {
        Some(IpAddr::from([8, 8, 8, last]))
    }

    struct Links {
        limiter: SigninLimiter,
        org: Id,
    }

    impl Links {
        fn new() -> Self {
            Self {
                limiter: signin_limiter(SigninLimits::default(), Vec::new()).0,
                org: Id::from_string("org"),
            }
        }

        async fn try_password(
            &self,
            link: &str,
            password: &str,
            source: Option<IpAddr>,
        ) -> Result<(), SigninError> {
            let share_id = Id::from_string(link);

            let account = SigninAccount::ResourceShare {
                share_id: &share_id,
                org_id: &self.org,
            };

            check_access_password(&self.limiter, &account, password, &HASH, source).await
        }

        /// 输错这么多次，每次都老实地说密码不对。
        async fn fail(&self, link: &str, times: u32, source: Option<IpAddr>) {
            for attempt in 1..=times {
                let answer = self.try_password(link, WRONG, source).await;

                assert!(
                    matches!(answer, Err(SigninError::Failed(Error::Unauthorized(_)))),
                    "attempt {attempt}: {answer:?}",
                );
            }
        }
    }

    fn refused(answer: Result<(), SigninError>) -> SigninRefused {
        match answer {
            Err(SigninError::Refused(refused)) => refused,
            other => panic!("should be refused: {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_right_password_opens_the_link() {
        let links = Links::new();

        assert!(links.try_password("a", RIGHT, from(1)).await.is_ok());
    }

    #[tokio::test]
    async fn the_spaces_around_a_password_are_not_part_of_it() {
        let links = Links::new();

        assert!(
            links
                .try_password("a", " open sesame\n", from(1))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn a_wrong_password_answers_with_its_own_code() {
        let response = wrong_password();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(body["code"], "share_password_invalid");
    }

    #[tokio::test]
    async fn five_wrong_passwords_lock_that_source_and_the_password_is_no_longer_checked() {
        let links = Links::new();

        links.fail("a", 5, from(1)).await;

        // 猜对了也看不出来。
        let wait = refused(links.try_password("a", RIGHT, from(1)).await).retry_after_secs;

        assert!((14 * 60..=15 * 60).contains(&wait), "{wait}");
    }

    #[tokio::test]
    async fn a_lock_stays_on_the_source_and_the_link() {
        let links = Links::new();

        links.fail("a", 5, from(1)).await;

        assert!(links.try_password("a", RIGHT, from(2)).await.is_ok());
        assert!(links.try_password("b", RIGHT, from(1)).await.is_ok());
    }

    #[tokio::test]
    async fn a_link_under_attack_is_paced_but_a_source_that_opened_it_still_gets_in() {
        let links = Links::new();

        assert!(links.try_password("a", RIGHT, from(1)).await.is_ok());

        // 每个来源只输错一次，到不了来源的锁。
        for attacker in 0..20 {
            links.fail("a", 1, from(100 + attacker)).await;
        }

        let wait = refused(links.try_password("a", RIGHT, from(2)).await).retry_after_secs;

        assert!((1..=60).contains(&wait), "{wait}");
        assert!(links.try_password("a", RIGHT, from(1)).await.is_ok());
    }

    #[tokio::test]
    async fn a_private_address_is_no_source_so_only_the_link_is_paced() {
        let links = Links::new();

        // 同一来源的 5 次在这里不算数：所有人都是代理的地址，锁住它就锁住了所有人。
        links.fail("a", 20, None).await;

        let wait = refused(links.try_password("a", RIGHT, None).await).retry_after_secs;

        assert!((1..=60).contains(&wait), "{wait}");
    }

    #[tokio::test]
    async fn opening_the_link_clears_what_that_source_got_wrong() {
        let links = Links::new();

        for _ in 0..2 {
            links.fail("a", 4, from(1)).await;

            assert!(links.try_password("a", RIGHT, from(1)).await.is_ok());
        }
    }

    #[tokio::test]
    async fn a_damaged_hash_is_our_fault_and_does_not_count_against_the_visitor() {
        let links = Links::new();
        let share_id = Id::from_string("a");

        let account = SigninAccount::ResourceShare {
            share_id: &share_id,
            org_id: &links.org,
        };

        for _ in 0..30 {
            let answer =
                check_access_password(&links.limiter, &account, RIGHT, "damaged", from(1)).await;

            assert!(
                matches!(answer, Err(SigninError::Failed(Error::Internal(_)))),
                "{answer:?}",
            );
        }
    }
}
