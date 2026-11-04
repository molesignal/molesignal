// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM issues：错误到达时开 issue，已解决的 issue 在更新的版本再现时判为回归，
//! 应用每小时新开的 issue 超出上限后并入「其余」issue。

mod common;

use std::time::{SystemTime, UNIX_EPOCH};

use molesignal::{
    domain::rum::{RumIssue, RumIssueChange, RumIssueStatus},
    infra::rum::issues::identity::{issue_id, overflow_id},
    shared::time::TimestampMicros,
};
use serde_json::{Map, Value, json};

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

async fn create_application(s: &common::TestServer, id: &str, settings: Value) {
    let (hk, hv) = s.auth_header();

    let resp = s
        .client
        .post(format!("{}/api/v1/rum/applications", s.base_url))
        .header(hk, &hv)
        .json(&json!({ "name": id, "id": id, "platform": "browser", "settings": settings }))
        .send()
        .await
        .unwrap();

    assert!(
        resp.status().is_success(),
        "application create failed: {}",
        resp.status()
    );
}

async fn send(s: &common::TestServer, events: Vec<Value>) {
    let (hk, hv) = s.auth_header();
    let sent = events.len();

    let resp = s
        .client
        .post(format!("{}/api/v1/rum/events", s.base_url))
        .header(hk, &hv)
        .json(&events)
        .send()
        .await
        .unwrap();

    assert!(
        resp.status().is_success(),
        "intake failed: {}",
        resp.status()
    );

    let body: Value = resp.json().await.unwrap();

    assert_eq!(body["accepted"], sent, "{body}");
}

/// A view: what tells the application it has this version.
fn view(application: &str, version: &str) -> Value {
    json!({
        "type": "view",
        "date": now_ms(),
        "application": { "id": application },
        "service": "web",
        "env": "prod",
        "version": version,
        "source": "browser",
        "session": { "id": "s-issues", "type": "user" },
        "view": {
            "id": format!("v-{version}"),
            "url": "https://shop.example.com/",
            "revision": 1,
            "time_spent": 1_000_000_000_i64,
        },
    })
}

fn error(application: &str, version: &str, message: &str) -> Value {
    json!({
        "type": "error",
        "date": now_ms(),
        "application": { "id": application },
        "service": "web",
        "env": "prod",
        "version": version,
        "source": "browser",
        "session": { "id": "s-issues", "type": "user" },
        "view": { "id": format!("v-{version}") },
        "error": {
            "id": format!("e-{version}-{}", message.len()),
            "message": message,
            "source": "source",
            "type": "TypeError",
        },
    })
}

/// The id the server gives the issue of an error like [`error`].
fn id_of(application: &str, message: &str) -> String {
    let mut row = Map::new();

    row.insert("application.id".into(), json!(application));
    row.insert("error.source".into(), json!("source"));
    row.insert("error.type".into(), json!("TypeError"));
    row.insert("error.message".into(), json!(message));

    issue_id(&row).expect("an id")
}

async fn issue(s: &common::TestServer, id: &str) -> Option<RumIssue> {
    s.state
        .telemetry
        .rum_issues
        .get(&s.root_org_id, id)
        .await
        .unwrap()
}

#[tokio::test]
async fn an_error_opens_an_issue_and_a_resolved_one_regresses_in_a_newer_version() {
    if common::skip_unless_enabled() {
        return;
    }

    let s = common::TestServer::start().await;
    let application = "issues-web";
    let message = "Cannot read properties of undefined (reading 'price')";
    let id = id_of(application, message);

    create_application(&s, application, json!({})).await;

    // The application has its first version, and the issue is there already, resolved.
    send(&s, vec![view(application, "1.0.0")]).await;

    let repository = &s.state.telemetry.rum_issues;
    let opened = RumIssue::opened(
        id.clone(),
        application.into(),
        TimestampMicros::now(),
        Some("1.0.0".into()),
    );

    repository
        .open(&s.root_org_id, std::slice::from_ref(&opened))
        .await
        .unwrap();

    let resolved = repository
        .update(
            &s.root_org_id,
            &id,
            &RumIssueChange {
                status: Some(RumIssueStatus::Resolved),
                ..Default::default()
            },
            &s.root_user_id,
            TimestampMicros::now(),
        )
        .await
        .unwrap()
        .expect("the issue is kept");

    assert_eq!(resolved.status, RumIssueStatus::Resolved);
    assert_eq!(resolved.resolved_version.as_deref(), Some("1.0.0"));

    // The same error in the version it was resolved in does not reopen it.
    send(&s, vec![error(application, "1.0.0", message)]).await;

    assert_eq!(
        issue(&s, &id).await.unwrap().status,
        RumIssueStatus::Resolved
    );

    // A newer version arrives, and the error comes back in it.
    send(&s, vec![view(application, "1.1.0")]).await;
    send(&s, vec![error(application, "1.1.0", message)]).await;

    let regressed = issue(&s, &id).await.unwrap();

    assert_eq!(regressed.status, RumIssueStatus::Regressed);
    assert_eq!(regressed.regressed_version.as_deref(), Some("1.1.0"));
    assert_eq!(regressed.last_version.as_deref(), Some("1.1.0"));
}

#[tokio::test]
async fn what_an_application_opens_past_its_hourly_limit_is_counted_as_one_issue() {
    if common::skip_unless_enabled() {
        return;
    }

    let s = common::TestServer::start().await;
    let application = "noisy-web";

    create_application(&s, application, json!({ "issuesPerHour": 1 })).await;

    send(
        &s,
        vec![
            error(application, "1.0.0", "first problem"),
            error(application, "1.0.0", "a second problem"),
            error(application, "1.0.0", "a third, different problem"),
        ],
    )
    .await;

    let first = issue(&s, &id_of(application, "first problem")).await;
    let second = issue(&s, &id_of(application, "a second problem")).await;
    let rest = issue(&s, &overflow_id(application)).await;

    assert!(first.is_some_and(|issue| !issue.overflow));
    assert!(second.is_none());
    assert!(rest.is_some_and(|issue| issue.overflow));
}
