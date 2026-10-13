// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! scheduled_reports webhook delivery 集成。
//!
//! happy: 起 wiremock 监听 POST → create report（正确 CreateReq schema）→ 复用 server 的
//! repo 直接构造 `ScheduledReportRunner` 并 `tick_once()` 触发投递 → wiremock 收到请求
//! (`expect(1)`) + `/deliveries` 的 `status=sent`、无 error。
//! sad: wiremock 返 500 → `status=failed` + `error` non-NULL。

mod common;

use std::sync::Arc;

use molesignal::{
    app::reporting::ScheduledReportIdentity,
    bootstrap::workers::scheduled_reports::ScheduledReportRunner,
};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path as wm_path},
};

/// 用正确的 `CreateReq` schema 建一个 webhook report，返回其 id。
async fn create_webhook_report(
    s: &common::TestServer,
    hk: &'static str,
    hv: &str,
    name: &str,
    webhook_url: &str,
) -> String {
    let create = s
        .client
        .post(format!("{}/api/v1/scheduled_reports", s.base_url))
        .header(hk, hv)
        .json(&serde_json::json!({
            "name": name,
            "dashboard_id": "dash-it",
            "cron": "every:1d",
            "format": "json",
            "recipients": [{ "kind": "webhook", "target": webhook_url }],
            "time_range_json": { "from": "now-7d", "to": "now" },
        }))
        .send()
        .await
        .unwrap();
    assert!(
        create.status().is_success(),
        "create report must succeed (got {})",
        create.status()
    );
    let body: serde_json::Value = create.json().await.unwrap();
    body["id"].as_str().expect("report id").to_string()
}

/// 复用 server 的同一套 repo / object_store / renderer 直接构造 runner 并触发一次
/// tick —— 新建报表 last_run_at=None 即 due，会立刻投递给 recipients。
async fn run_reports_tick(s: &common::TestServer) {
    let runner = ScheduledReportRunner::new(
        s.state.platform.scheduled_reports.clone(),
        s.state.storage.object_store.clone(),
        s.state.platform.report_renderer.clone(),
        s.state.platform.report_renderer_base_url.clone(),
        Arc::new(ScheduledReportIdentity::new(
            s.state.iam.service.clone(),
            s.state.iam.access.clone(),
        )),
    );
    runner
        .tick_once()
        .await
        .expect("scheduled_reports tick_once");
}

/// 取该报表的第一条投递记录。
async fn latest_delivery(
    s: &common::TestServer,
    hk: &'static str,
    hv: &str,
    id: &str,
) -> serde_json::Value {
    let resp = s
        .client
        .get(format!(
            "{}/api/v1/scheduled_reports/{id}/deliveries",
            s.base_url
        ))
        .header(hk, hv)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "deliveries must be 200");
    let arr: serde_json::Value = resp.json().await.unwrap();
    let list = arr.as_array().expect("deliveries array");
    assert!(!list.is_empty(), "at least one delivery must be recorded");
    list[0].clone()
}

#[tokio::test]
async fn webhook_delivery_records_sent_status() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wm_path("/webhook"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;
    let webhook_url = format!("{}/webhook", mock_server.uri());

    let id = create_webhook_report(&s, hk, &hv, "webhook-sent-it", &webhook_url).await;
    run_reports_tick(&s).await;

    // 投递成功（webhook 返 200）→ deliveries.status == sent，无 error。
    let d = latest_delivery(&s, hk, &hv, &id).await;
    assert_eq!(d["status"], "sent", "200 webhook → sent");
    assert!(d["error"].is_null(), "successful delivery has no error");
    assert_eq!(d["recipient_kind"], "webhook");

    // drop 触发 wiremock 的 expect(1) 校验：webhook 确实被打了一次。
    drop(mock_server);
}

#[tokio::test]
async fn webhook_500_records_failed_status() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let mock_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wm_path("/webhook"))
        .respond_with(ResponseTemplate::new(500))
        .expect(1)
        .mount(&mock_server)
        .await;
    let webhook_url = format!("{}/webhook", mock_server.uri());

    let id = create_webhook_report(&s, hk, &hv, "webhook-fail-it", &webhook_url).await;
    run_reports_tick(&s).await;

    // 投递失败（webhook 返 500）→ deliveries.status == failed，error non-null。
    let d = latest_delivery(&s, hk, &hv, &id).await;
    assert_eq!(d["status"], "failed", "500 webhook → failed");
    assert!(d["error"].is_string(), "failed delivery records an error");

    drop(mock_server);
}

#[tokio::test]
async fn preview_returns_rendered_payload() {
    // GET /scheduled_reports/{id}/preview 即时渲染报表预览（取代前端占位）。
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let create = s
        .client
        .post(format!("{}/api/v1/scheduled_reports", s.base_url))
        .header(hk, &hv)
        // CreateReq schema：cron + recipients:[{kind,target}] + 恰好一个 dashboard_id/saved_view_id。
        .json(&serde_json::json!({
            "name": "preview-it",
            "dashboard_id": "dash-preview-it",
            "cron": "every:1d",
            "format": "json",
            "recipients": [{ "kind": "webhook", "target": "https://example.com/webhook" }],
            "time_range_json": { "from": "now-7d", "to": "now" },
        }))
        .send()
        .await
        .unwrap();
    assert!(
        create.status().is_success(),
        "create report must succeed (got {})",
        create.status()
    );
    let body: serde_json::Value = create.json().await.unwrap();
    let id = body["id"].as_str().expect("report id");

    let resp = s
        .client
        .get(format!(
            "{}/api/v1/scheduled_reports/{id}/preview",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "preview must be 200");
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        ct.contains("application/json"),
        "json format → json content-type"
    );

    let preview: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(preview["format"], "json");
    assert_eq!(preview["report_id"], id);
    assert!(preview.get("time_range").is_some());
}

#[tokio::test]
async fn pdf_preview_never_returns_json_as_application_pdf() {
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let create = s
        .client
        .post(format!("{}/api/v1/scheduled_reports", s.base_url))
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "pdf-preview-it",
            "dashboard_id": "dash-pdf-preview-it",
            "cron": "every:1d",
            "format": "pdf",
            "recipients": [{ "kind": "webhook", "target": "https://example.com/webhook" }],
            "time_range_json": { "from": "now-7d", "to": "now" },
        }))
        .send()
        .await
        .unwrap();
    assert!(create.status().is_success());
    let body: serde_json::Value = create.json().await.unwrap();
    let id = body["id"].as_str().expect("report id");

    // TestServer 默认不启用 Chrome renderer。此时必须明确返回 503 JSON 错误，
    // 不能返回 200 application/pdf + JSON body。
    let response = s
        .client
        .get(format!(
            "{}/api/v1/scheduled_reports/{id}/preview",
            s.base_url
        ))
        .header(hk, &hv)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 503);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    assert!(content_type.contains("application/json"));
    let error: serde_json::Value = response.json().await.unwrap();
    assert_eq!(error["error"], "unavailable");
}

#[tokio::test]
async fn a_report_records_the_signed_in_user_who_saved_it() {
    // 定时投递的 png / pdf 以最后保存报表的登录用户的身份渲染；这位用户随每次保存写入。
    if common::skip_unless_enabled() {
        return;
    }
    let s = common::TestServer::start().await;
    let (hk, hv) = s.auth_header();

    let profile: serde_json::Value = s
        .client
        .get(format!("{}/api/v1/me/profile", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let me = profile["user_id"].as_str().expect("signed-in user id");

    let id = create_webhook_report(&s, hk, &hv, "run-as-it", "https://example.com/webhook").await;
    let report_url = format!("{}/api/v1/scheduled_reports/{id}", s.base_url);

    let stored: serde_json::Value = s
        .client
        .get(&report_url)
        .header(hk, &hv)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(stored["run_as_user_id"], me);

    // 这位用户的显示名和邮箱随报表返回，界面不必再去读成员目录。
    let named = serde_json::json!({
        "id": me,
        "display_name": profile["display_name"],
        "email": profile["email"],
    });
    assert_eq!(stored["run_as_user"], named);

    let listed: serde_json::Value = s
        .client
        .get(format!("{}/api/v1/scheduled_reports", s.base_url))
        .header(hk, &hv)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let listed = listed
        .as_array()
        .expect("a list of reports")
        .iter()
        .find(|report| report["id"] == id.as_str())
        .expect("the new report is listed");
    assert_eq!(listed["run_as_user"], named);

    // 再保存一次（暂停）：身份是这次保存的人，其余字段照请求写入。
    let update = s
        .client
        .put(&report_url)
        .header(hk, &hv)
        .json(&serde_json::json!({
            "name": "run-as-it",
            "dashboard_id": "dash-it",
            "cron": "every:1d",
            "format": "json",
            "recipients": [{ "kind": "webhook", "target": "https://example.com/webhook" }],
            "time_range_json": { "from": "now-7d", "to": "now" },
            "enabled": false,
        }))
        .send()
        .await
        .unwrap();
    assert!(
        update.status().is_success(),
        "update report must succeed (got {})",
        update.status()
    );
    let updated: serde_json::Value = update.json().await.unwrap();
    assert_eq!(updated["run_as_user"], named);

    let stored: serde_json::Value = s
        .client
        .get(&report_url)
        .header(hk, &hv)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(stored["run_as_user_id"], me);
    assert_eq!(stored["enabled"], false);
}
