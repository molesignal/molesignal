// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod common;

use common::{TestServer, set_cookie, skip_unless_enabled};
use molesignal::app::iam::mfa::totp::{code_at, step_at};
use reqwest::StatusCode;
use serde_json::{Value, json};

#[tokio::test]
async fn an_account_with_an_authenticator_signs_in_in_two_steps() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }

    let s = TestServer::start().await;
    let api = format!("{}/api/v1", s.base_url);
    let credentials = json!({"email": s.root_email, "password": s.root_password});

    let first = s
        .client
        .post(format!("{api}/auth/signin"))
        .json(&credentials)
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let browser = cookie_of(&first);

    // Adding a factor needs the owner: a session alone is not enough.
    let status: Value = s
        .client
        .get(format!("{api}/me/mfa"))
        .header("cookie", &browser)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["addingRequires"], "password");
    assert_eq!(status["passkeys"], json!([]));

    // The test server listens on an IP address, which a passkey cannot be bound to.
    assert_eq!(status["passkeysUnavailable"], "ip_address");

    let passkey_signin = s
        .client
        .post(format!("{api}/auth/signin/passkey/options"))
        .send()
        .await
        .unwrap();
    assert_eq!(passkey_signin.status(), StatusCode::BAD_REQUEST);

    let without_proof = s
        .client
        .post(format!("{api}/me/mfa/totp/setup"))
        .header("cookie", &browser)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(without_proof.status(), StatusCode::BAD_REQUEST);

    let wrong_password = s
        .client
        .post(format!("{api}/me/mfa/totp/setup"))
        .header("cookie", &browser)
        .json(&json!({"proof": {"method": "password", "password": "not the password"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_password.status(), StatusCode::BAD_REQUEST);

    // Turn the authenticator on: the first code from it confirms the setup.
    let setup: Value = s
        .client
        .post(format!("{api}/me/mfa/totp/setup"))
        .header("cookie", &browser)
        .json(&json!({"proof": {"method": "password", "password": s.root_password}}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(setup["qrSvg"].as_str().unwrap().contains("<svg"));
    assert!(
        setup["otpauthUrl"]
            .as_str()
            .unwrap()
            .starts_with("otpauth://totp/")
    );

    let secret = base32_decode(setup["secret"].as_str().unwrap());
    let step = step_at(chrono::Utc::now().timestamp());

    let confirmed = s
        .client
        .post(format!("{api}/me/mfa/totp/confirm"))
        .header("cookie", &browser)
        .json(&json!({"setup": setup["setup"], "code": code_at(&secret, step)}))
        .send()
        .await
        .unwrap();
    assert_eq!(confirmed.status(), StatusCode::OK);
    let reconfirmed = cookie_of(&confirmed);
    let body: Value = confirmed.json().await.unwrap();
    let recovery: Vec<String> = serde_json::from_value(body["recoveryCodes"].clone()).unwrap();
    assert_eq!(recovery.len(), 10);

    // Turning it on ended the other sign-ins; the browser that did it was reconfirmed.
    let dashboards = format!("{api}/dashboards");
    let old = s
        .client
        .get(&dashboards)
        .header("cookie", &browser)
        .send()
        .await
        .unwrap();
    assert_eq!(old.status(), StatusCode::UNAUTHORIZED);
    let kept = s
        .client
        .get(&dashboards)
        .header("cookie", &reconfirmed)
        .send()
        .await
        .unwrap();
    assert_eq!(kept.status(), StatusCode::OK);

    // The password alone now gets a challenge, not a session.
    let challenge = password_step(&s, &api, &credentials).await;

    let wrong = verify(&s, &api, &challenge, "totp", "000000").await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

    // The code that confirmed the setup cannot sign in again; the next one can.
    let replayed = verify(&s, &api, &challenge, "totp", &code_at(&secret, step)).await;
    assert_eq!(replayed.status(), StatusCode::UNAUTHORIZED);

    let next = verify(&s, &api, &challenge, "totp", &code_at(&secret, step + 1)).await;
    assert_eq!(next.status(), StatusCode::OK);
    assert!(set_cookie(&next, "accessToken").is_some());
    let signed_in: Value = next.json().await.unwrap();
    assert_eq!(signed_in["login"]["userId"], s.root_user_id.0);

    // A recovery code signs in once.
    let challenge = password_step(&s, &api, &credentials).await;
    let recovered = verify(&s, &api, &challenge, "recovery_code", &recovery[0]).await;
    assert_eq!(recovered.status(), StatusCode::OK);
    let recovered: Value = recovered.json().await.unwrap();
    assert_eq!(recovered["recoveryCodesLeft"], 9);

    let challenge = password_step(&s, &api, &credentials).await;
    let reused = verify(&s, &api, &challenge, "recovery_code", &recovery[0]).await;
    assert_eq!(reused.status(), StatusCode::UNAUTHORIZED);

    // Turning it off needs one more code; afterwards the password signs in on its own.
    let disabled = s
        .client
        .post(format!("{api}/me/mfa/totp/disable"))
        .header("cookie", &reconfirmed)
        .json(&json!({"method": "recovery_code", "code": recovery[1]}))
        .send()
        .await
        .unwrap();
    assert_eq!(disabled.status(), StatusCode::NO_CONTENT);

    let plain: Value = s
        .client
        .post(format!("{api}/auth/signin"))
        .json(&credentials)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(plain["login"]["userId"], s.root_user_id.0);
}

#[tokio::test]
async fn an_organization_that_asks_for_a_second_factor_gets_it_at_sign_in() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }

    let s = TestServer::start().await;
    let api = format!("{}/api/v1", s.base_url);
    let credentials = json!({"email": s.root_email, "password": s.root_password});

    let first = s
        .client
        .post(format!("{api}/auth/signin"))
        .json(&credentials)
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let browser = cookie_of(&first);
    let refresh = format!(
        "refreshToken={}",
        set_cookie(&first, "refreshToken").expect("refresh cookie")
    );

    // Single sign-on only needs a provider that can sign in.
    let without_provider = s
        .client
        .put(format!("{api}/settings/signin-policy"))
        .header("cookie", &browser)
        .json(&json!({"requireMfa": false, "ssoOnly": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(without_provider.status(), StatusCode::BAD_REQUEST);

    let required = s
        .client
        .put(format!("{api}/settings/signin-policy"))
        .header("cookie", &browser)
        .json(&json!({"requireMfa": true, "ssoOnly": false}))
        .send()
        .await
        .unwrap();
    assert_eq!(required.status(), StatusCode::OK);

    // The session that came in on a password alone ends at its next renewal.
    let renewed = s
        .client
        .post(format!("{api}/auth/refresh"))
        .header("cookie", &refresh)
        .send()
        .await
        .unwrap();
    assert_eq!(renewed.status(), StatusCode::UNAUTHORIZED);

    // The next sign-in sets a second factor up before it gets a session.
    let again = s
        .client
        .post(format!("{api}/auth/signin"))
        .json(&credentials)
        .send()
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::OK);
    assert!(set_cookie(&again, "accessToken").is_none());

    let body: Value = again.json().await.unwrap();
    let challenge = body["enroll"]["challenge"].as_str().expect("enrollment");

    let setup: Value = s
        .client
        .post(format!("{api}/auth/signin/enroll/totp"))
        .json(&json!({"challenge": challenge}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let secret = base32_decode(setup["secret"].as_str().unwrap());
    let step = step_at(chrono::Utc::now().timestamp());

    let enrolled = s
        .client
        .post(format!("{api}/auth/signin/enroll/totp/confirm"))
        .json(&json!({
            "challenge": challenge,
            "setup": setup["setup"],
            "code": code_at(&secret, step),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(enrolled.status(), StatusCode::OK);
    let browser = cookie_of(&enrolled);
    let refresh = format!(
        "refreshToken={}",
        set_cookie(&enrolled, "refreshToken").expect("refresh cookie")
    );

    let body: Value = enrolled.json().await.unwrap();
    assert_eq!(body["login"]["userId"], s.root_user_id.0);
    let recovery = body["recoveryCodes"].as_array().expect("recovery codes");
    assert_eq!(recovery.len(), 10);

    // This session passes, and the challenge is used up.
    let renewed = s
        .client
        .post(format!("{api}/auth/refresh"))
        .header("cookie", &refresh)
        .send()
        .await
        .unwrap();
    assert_eq!(renewed.status(), StatusCode::NO_CONTENT);

    let replayed = s
        .client
        .post(format!("{api}/auth/signin/enroll/totp"))
        .json(&json!({"challenge": challenge}))
        .send()
        .await
        .unwrap();
    assert_eq!(replayed.status(), StatusCode::UNAUTHORIZED);

    // The organization keeps the account from dropping its last second factor.
    let status: Value = s
        .client
        .get(format!("{api}/me/mfa"))
        .header("cookie", &browser)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(status["requiredBy"].as_array().map(Vec::len), Some(1));

    let dropped = s
        .client
        .post(format!("{api}/me/mfa/totp/disable"))
        .header("cookie", &browser)
        .json(&json!({"method": "recovery_code", "code": recovery[0]}))
        .send()
        .await
        .unwrap();
    assert_eq!(dropped.status(), StatusCode::BAD_REQUEST);

    // The second step comes back for the next password sign-in.
    let challenge = password_step(&s, &api, &credentials).await;
    assert!(!challenge.is_empty());
}

#[tokio::test]
async fn signing_out_other_devices_keeps_this_one() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }

    let s = TestServer::start().await;
    let api = format!("{}/api/v1", s.base_url);
    let credentials = json!({"email": s.root_email, "password": s.root_password});

    let mut browsers = Vec::new();

    for _ in 0..2 {
        let response = s
            .client
            .post(format!("{api}/auth/signin"))
            .json(&credentials)
            .send()
            .await
            .unwrap();

        browsers.push(cookie_of(&response));
    }

    let revoked = s
        .client
        .post(format!("{api}/me/sessions/revoke-others"))
        .header("cookie", &browsers[1])
        .send()
        .await
        .unwrap();
    assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
    let this_one = cookie_of(&revoked);

    let dashboards = format!("{api}/dashboards");

    for (cookie, expected) in [
        (&browsers[0], StatusCode::UNAUTHORIZED),
        (&this_one, StatusCode::OK),
    ] {
        let response = s
            .client
            .get(&dashboards)
            .header("cookie", cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
}

fn cookie_of(response: &reqwest::Response) -> String {
    format!(
        "accessToken={}",
        set_cookie(response, "accessToken").expect("access cookie")
    )
}

async fn password_step(s: &TestServer, api: &str, credentials: &Value) -> String {
    let response = s
        .client
        .post(format!("{api}/auth/signin"))
        .json(credentials)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(set_cookie(&response, "accessToken").is_none());

    let body: Value = response.json().await.unwrap();
    body["mfa"]["challenge"]
        .as_str()
        .expect("a second step")
        .to_owned()
}

async fn verify(
    s: &TestServer,
    api: &str,
    challenge: &str,
    method: &str,
    code: &str,
) -> reqwest::Response {
    s.client
        .post(format!("{api}/auth/signin/verify"))
        .json(&json!({"challenge": challenge, "method": method, "code": code}))
        .send()
        .await
        .unwrap()
}

fn base32_decode(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

    let mut bytes = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0u32;

    for symbol in text.bytes().filter(|byte| !byte.is_ascii_whitespace()) {
        let value = ALPHABET
            .iter()
            .position(|candidate| *candidate == symbol)
            .expect("base32 symbol") as u32;

        buffer = (buffer << 5) | value;
        bits += 5;

        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
        }
    }

    bytes
}
