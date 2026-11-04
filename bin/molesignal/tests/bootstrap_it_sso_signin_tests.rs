// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 测试登录走真实的回调：身份源是这里起的假服务，看结果记下来了，账号和绑定都没有变化。

mod common;

use std::collections::HashMap;

use axum::{
    Form, Json, Router,
    http::StatusCode as AxumStatus,
    routing::{get, post},
};
use common::{TestServer, skip_unless_enabled};
use molesignal::shared::{Error, ids::Id};
use reqwest::{StatusCode, header, redirect::Policy};
use serde_json::{Value, json};
use url::Url;

const CLIENT_SECRET: &str = "s3cret-value-of-the-client";
const IDENTITY_EMAIL: &str = "new.person@example.com";
const SUBJECT: &str = "fake-idp-user-1";

/// 只有 `good` 这个授权码能换到令牌；别的码，答复里还把 Client Secret 念出来。
async fn token(Form(form): Form<HashMap<String, String>>) -> (AxumStatus, Json<Value>) {
    if form.get("code").map(String::as_str) == Some("good") {
        return (
            AxumStatus::OK,
            Json(json!({ "access_token": "fake-token" })),
        );
    }

    let secret = form.get("client_secret").cloned().unwrap_or_default();

    (
        AxumStatus::UNAUTHORIZED,
        Json(json!({
            "error": "invalid_client",
            "error_description": format!("client secret {secret} is not valid"),
        })),
    )
}

async fn fake_identity_provider() -> String {
    let app = Router::new().route("/token", post(token)).route(
        "/userinfo",
        get(|| async {
            Json(json!({
                "id": SUBJECT,
                "email": IDENTITY_EMAIL,
                "name": "New Person",
                "groups": ["sre"],
            }))
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{address}")
}

struct Browser {
    api: String,
    cookie: String,
    client: reqwest::Client,
}

impl Browser {
    fn of(server: &TestServer) -> Self {
        Self {
            api: format!("{}/api/v1", server.base_url),
            cookie: format!("accessToken={}", server.root_token),
            client: reqwest::Client::builder()
                .redirect(Policy::none())
                .build()
                .unwrap(),
        }
    }

    async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{path}", self.api))
            .header(header::COOKIE, &self.cookie)
            .send()
            .await
            .unwrap()
    }

    async fn post(&self, path: &str, body: &Value) -> reqwest::Response {
        self.client
            .post(format!("{}{path}", self.api))
            .header(header::COOKIE, &self.cookie)
            .json(body)
            .send()
            .await
            .unwrap()
    }

    async fn put(&self, path: &str, body: &Value) -> reqwest::Response {
        self.client
            .put(format!("{}{path}", self.api))
            .header(header::COOKIE, &self.cookie)
            .json(body)
            .send()
            .await
            .unwrap()
    }

    async fn json(&self, path: &str) -> Value {
        let answer = self.get(path).await;
        assert_eq!(answer.status(), StatusCode::OK, "{path}");

        answer.json().await.unwrap()
    }

    /// 发起一次测试，再按身份源的样子带着授权码回来。回答回调的响应和这条测试的 id。
    async fn run_test(&self, provider_id: &str, code: &str) -> (reqwest::Response, String) {
        let started: Value = self
            .post(&format!("/sso/providers/{provider_id}/tests"), &json!({}))
            .await
            .json()
            .await
            .unwrap();

        let authorization_url = Url::parse(started["authorizationUrl"].as_str().unwrap()).unwrap();

        let state = authorization_url
            .query_pairs()
            .find(|(name, _)| name == "state")
            .map(|(_, value)| value.into_owned())
            .unwrap();

        // 回调从身份源那边回来，不带这个浏览器的 Cookie 也得能走完。
        let callback = Url::parse_with_params(
            &format!("{}/auth/sso/callback", self.api),
            &[("code", code), ("state", state.as_str())],
        )
        .unwrap();

        let answer = self.client.get(callback).send().await.unwrap();

        (answer, started["id"].as_str().unwrap().to_owned())
    }
}

fn oauth2_provider(idp: &str, scopes: &[&str]) -> Value {
    json!({
        "name": "Fake IdP",
        "kind": "oauth2",
        "enabled": false,
        "oauth2": {
            "authorizeUrl": format!("{idp}/authorize"),
            "tokenUrl": format!("{idp}/token"),
            "userinfoUrl": format!("{idp}/userinfo"),
            "clientId": "molesignal",
            "clientSecret": CLIENT_SECRET,
            "redirectUri": "http://127.0.0.1/api/v1/auth/sso/callback",
            "scopes": scopes,
        },
    })
}

#[tokio::test]
async fn a_test_sign_in_reports_and_leaves_no_account_behind() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }

    let server = TestServer::start().await;
    let idp = fake_identity_provider().await;
    let browser = Browser::of(&server);

    let created: Value = browser
        .post("/sso/providers", &oauth2_provider(&idp, &["profile"]))
        .await
        .json()
        .await
        .unwrap();
    let provider_id = created["id"].as_str().unwrap().to_owned();

    let listed = browser.json("/sso/providers").await;
    assert_eq!(listed[0]["test"]["status"], "untested");

    // 停用的服务也能测：回调不走登录那条路，不会说它停用了。
    let (answer, test_id) = browser.run_test(&provider_id, "good").await;
    assert_eq!(answer.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        answer.headers()[header::LOCATION],
        format!("/iam/sso/tests/{test_id}").as_str()
    );
    assert!(answer.headers().get(header::SET_COOKIE).is_none());

    let test = browser.json(&format!("/sso/tests/{test_id}")).await;
    assert_eq!(test["status"], "passed");
    assert_eq!(test["current"], true);
    assert_eq!(test["startedByYou"], true);
    assert_eq!(test["identity"]["email"], IDENTITY_EMAIL);
    assert_eq!(test["identity"]["groups"], json!(["sre"]));
    assert_eq!(test["mapping"]["account"], "create");
    assert_eq!(test["mapping"]["isTester"], false);
    assert_eq!(test["mapping"]["roleSource"], "signup");

    // 什么都没留下：没有开户，没有绑定。
    let users = &server.state.iam.service.users;
    assert!(matches!(
        users.get_by_email(IDENTITY_EMAIL).await,
        Err(Error::NotFound(_))
    ));

    let bound = server
        .state
        .iam
        .sso_identities
        .find(&Id::from_string(provider_id.clone()), SUBJECT)
        .await
        .unwrap();
    assert!(bound.is_none());

    let listed = browser.json("/sso/providers").await;
    assert_eq!(listed[0]["test"]["status"], "passed");
    assert_eq!(listed[0]["test"]["testId"], test_id.as_str());

    // 一次失败：说得清类别，身份源念出来的密钥不进结果。
    let (answer, failed_id) = browser.run_test(&provider_id, "bad").await;
    assert_eq!(answer.status(), StatusCode::SEE_OTHER);

    let failed = browser.json(&format!("/sso/tests/{failed_id}")).await;
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["failure"]["code"], "token_exchange");
    assert!(failed.get("identity").is_none());

    let detail = failed["failure"]["detail"].as_str().unwrap();
    assert!(
        detail.contains("invalid") || detail.contains("401"),
        "{detail}"
    );
    assert!(!failed.to_string().contains(CLIENT_SECRET));

    let listed = browser.json("/sso/providers").await;
    assert_eq!(listed[0]["test"]["status"], "failed");

    // 配置改了，之前的结果作废。
    let saved = browser
        .put(
            &format!("/sso/providers/{provider_id}"),
            &oauth2_provider(&idp, &["profile", "email"]),
        )
        .await;
    assert_eq!(saved.status(), StatusCode::OK);

    let listed = browser.json("/sso/providers").await;
    assert_eq!(listed[0]["test"]["status"], "stale");

    let voided = browser.json(&format!("/sso/tests/{test_id}")).await;
    assert_eq!(voided["status"], "passed");
    assert_eq!(voided["current"], false);
}

#[tokio::test]
async fn a_directory_that_cannot_be_reached_says_so_and_the_password_stays_out() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }

    let server = TestServer::start().await;
    let browser = Browser::of(&server);

    let created: Value = browser
        .post(
            "/sso/providers",
            &json!({
                "name": "Directory",
                "kind": "ldap",
                "enabled": false,
                "ldap": {
                    "url": "ldaps://127.0.0.1:1",
                    "baseDn": "dc=example,dc=com",
                    "userFilter": "(mail={username})",
                },
            }),
        )
        .await
        .json()
        .await
        .unwrap();
    let provider_id = created["id"].as_str().unwrap();

    let password = "typed-into-the-dialog";

    let answer = browser
        .post(
            &format!("/sso/providers/{provider_id}/tests/ldap"),
            &json!({ "username": "alice", "password": password }),
        )
        .await;
    assert_eq!(answer.status(), StatusCode::OK);

    let test: Value = answer.json().await.unwrap();
    assert_eq!(test["status"], "failed");
    assert_eq!(test["failure"]["code"], "ldap_connect");
    assert!(!test.to_string().contains(password));

    // 重定向式的接口不收目录服务。
    let refused = browser
        .post(&format!("/sso/providers/{provider_id}/tests"), &json!({}))
        .await;
    assert_eq!(refused.status(), StatusCode::BAD_REQUEST);

    let empty = browser
        .post(
            &format!("/sso/providers/{provider_id}/tests/ldap"),
            &json!({ "username": "alice", "password": "" }),
        )
        .await;
    assert_eq!(empty.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn tests_of_a_provider_are_limited_and_anyone_elses_are_out_of_reach() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }

    let server = TestServer::start().await;
    let idp = fake_identity_provider().await;
    let browser = Browser::of(&server);

    let created: Value = browser
        .post("/sso/providers", &oauth2_provider(&idp, &[]))
        .await
        .json()
        .await
        .unwrap();
    let provider_id = created["id"].as_str().unwrap();

    let path = format!("/sso/providers/{provider_id}/tests");

    for _ in 0..10 {
        let started = browser.post(&path, &json!({})).await;
        assert_eq!(started.status(), StatusCode::OK);
    }

    let refused = browser.post(&path, &json!({})).await;
    assert_eq!(refused.status(), StatusCode::TOO_MANY_REQUESTS);

    let missing = browser.get("/sso/tests/no-such-test").await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let anonymous = reqwest::Client::new()
        .get(format!("{}/sso/tests/no-such-test", browser.api))
        .send()
        .await
        .unwrap();
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
}
