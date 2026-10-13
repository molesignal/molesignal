// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Status-page logo upload, authenticated preview, and anonymous delivery end to end.

mod common;

use reqwest::StatusCode;
use serde_json::{Value, json};

fn page_settings(slug: &str, logo_url: Option<&str>, visibility: &str) -> Value {
    json!({
        "name": "Acme Customer Status",
        "slug": slug,
        "logo_url": logo_url,
        "brand_color": "#4F46E5",
        "timezone": "UTC",
        "language": "en-us",
        "languages": ["en-us", "zh-cn"],
        "history_days": 30,
        "visibility": visibility
    })
}

#[tokio::test]
async fn component_history_and_rss_are_published_without_exposing_request_time_as_an_update() {
    if common::skip_unless_enabled() {
        return;
    }
    let server = common::TestServer::start().await;
    let (header_name, header_value) = server.auth_header();

    let create = server
        .client
        .post(format!("{}/api/v1/status-pages", server.base_url))
        .header(header_name, &header_value)
        .json(&page_settings("acme-history", None, "public"))
        .send()
        .await
        .expect("create history status page");
    assert_eq!(create.status(), StatusCode::CREATED);
    let created: Value = create.json().await.expect("created history page body");
    let page_id = created["id"].as_str().expect("page id");

    let component = server
        .client
        .post(format!(
            "{}/api/v1/status-pages/{page_id}/components",
            server.base_url
        ))
        .header(header_name, &header_value)
        .json(&json!({
            "name": "API",
            "description": "Customer API",
            "status": "operational"
        }))
        .send()
        .await
        .expect("create status component");
    assert_eq!(component.status(), StatusCode::CREATED);
    let component: Value = component.json().await.expect("component body");
    let component_id = component["id"].as_str().expect("component id");

    let update = server
        .client
        .put(format!(
            "{}/api/v1/status-pages/{page_id}/components/{component_id}",
            server.base_url
        ))
        .header(header_name, &header_value)
        .json(&json!({
            "name": "API",
            "description": "Customer API",
            "status": "degraded_performance"
        }))
        .send()
        .await
        .expect("update status component");
    assert_eq!(update.status(), StatusCode::OK);

    let public = server
        .client
        .get(format!(
            "{}/api/v1/public/status-pages/acme-history",
            server.base_url
        ))
        .send()
        .await
        .expect("read public history");
    assert_eq!(public.status(), StatusCode::OK);
    let snapshot: Value = public.json().await.expect("public history body");
    let events = snapshot["component_status_events"]
        .as_array()
        .expect("component status events");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["status"], "operational");
    assert!(events[0]["ended_at"].is_number());
    assert_eq!(events[1]["status"], "degraded_performance");
    assert!(events[1]["ended_at"].is_null());
    assert!(snapshot["updated_at"].as_i64() <= snapshot["generated_at"].as_i64());

    let rss = server
        .client
        .get(format!(
            "{}/api/v1/public/status-pages/acme-history/feed.rss",
            server.base_url
        ))
        .send()
        .await
        .expect("read public RSS feed");
    assert_eq!(rss.status(), StatusCode::OK);
    assert_eq!(
        rss.headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("application/rss+xml; charset=utf-8")
    );
    assert!(
        rss.text()
            .await
            .expect("RSS body")
            .contains("Acme Customer Status")
    );
}

#[tokio::test]
async fn custom_domain_routes_only_after_dns_and_tls_activation() {
    if common::skip_unless_enabled() {
        return;
    }
    let server = common::TestServer::start().await;
    let (header_name, header_value) = server.auth_header();
    let domain = "status.acme-domain.example";

    let create = server
        .client
        .post(format!("{}/api/v1/status-pages", server.base_url))
        .header(header_name, &header_value)
        .json(&page_settings("acme-domain", None, "public"))
        .send()
        .await
        .expect("create custom-domain status page");
    assert_eq!(create.status(), StatusCode::CREATED);
    let created: Value = create.json().await.expect("created status page body");
    let page_id = created["id"].as_str().expect("page id");

    let configure = server
        .client
        .put(format!(
            "{}/api/v1/status-pages/{page_id}/domain",
            server.base_url
        ))
        .header(header_name, &header_value)
        .json(&json!({"hostname": domain}))
        .send()
        .await
        .expect("configure custom domain");
    assert_eq!(configure.status(), StatusCode::OK);
    let configuration: Value = configure.json().await.expect("domain configuration");
    assert_eq!(configuration["config"]["state"], "pending_dns");

    let pending_domain = server
        .client
        .get(format!(
            "{}/api/v1/public/status-pages/by-domain/current",
            server.base_url
        ))
        .header("host", "STATUS.ACME-DOMAIN.EXAMPLE:443")
        .send()
        .await
        .expect("reject pending custom-domain routing");
    assert_eq!(pending_domain.status(), StatusCode::NO_CONTENT);

    // DNS and ACME are external systems. Simulate their persisted terminal
    // state after verifying that pending configuration cannot route.
    let pool = sqlx::PgPool::connect(&server.settings.meta_database.dsn)
        .await
        .expect("connect status-page test pool");
    let domain_id = molesignal::shared::ids::Id::new();
    let now = molesignal::shared::time::TimestampMicros::now().0;
    sqlx::query(
        "INSERT INTO domains
            (id, org_id, hostname, state, cert_pem, cert_not_after_micros,
             last_error, created_at_micros, updated_at_micros)
         VALUES ($1, $2, $3, 'active', 'test-certificate', $4, NULL, $5, $5)",
    )
    .bind(&domain_id.0)
    .bind(&server.root_org_id.0)
    .bind(domain)
    .bind(now + 90 * 24 * 60 * 60 * 1_000_000_i64)
    .bind(now)
    .execute(&pool)
    .await
    .expect("activate ACME domain");
    sqlx::query(
        "UPDATE status_page_domain_configs
         SET state = 'verified', domain_id = $3, routing_valid = TRUE,
             last_checked_at_micros = $4, updated_at_micros = $4
         WHERE org_id = $1 AND status_page_id = $2",
    )
    .bind(&server.root_org_id.0)
    .bind(page_id)
    .bind(&domain_id.0)
    .bind(now)
    .execute(&pool)
    .await
    .expect("link active custom domain");

    let by_domain = server
        .client
        .get(format!(
            "{}/api/v1/public/status-pages/by-domain/current",
            server.base_url
        ))
        .header("host", "STATUS.ACME-DOMAIN.EXAMPLE:443")
        .send()
        .await
        .expect("resolve active public page by host");
    assert_eq!(by_domain.status(), StatusCode::OK);
    let snapshot: Value = by_domain.json().await.expect("public domain snapshot");
    assert_eq!(snapshot["page"]["slug"], "acme-domain");
    assert_eq!(snapshot["page"]["history_days"], 30);
    assert!(snapshot["page"].get("org_id").is_none());
    assert!(snapshot["page"].get("custom_domain").is_none());

    let by_slug = server
        .client
        .get(format!(
            "{}/api/v1/public/status-pages/acme-domain",
            server.base_url
        ))
        .send()
        .await
        .expect("resolve public page by slug");
    assert_eq!(by_slug.status(), StatusCode::OK);

    let unmatched_domain = server
        .client
        .get(format!(
            "{}/api/v1/public/status-pages/by-domain/current",
            server.base_url
        ))
        .header("host", "status.unknown.example")
        .send()
        .await
        .expect("resolve unassigned host");
    assert_eq!(unmatched_domain.status(), StatusCode::NO_CONTENT);

    let make_private = server
        .client
        .put(format!("{}/api/v1/status-pages/{page_id}", server.base_url))
        .header(header_name, &header_value)
        .json(&page_settings("acme-domain", None, "private"))
        .send()
        .await
        .expect("make custom-domain page private");
    assert_eq!(make_private.status(), StatusCode::OK);

    let private_domain = server
        .client
        .get(format!(
            "{}/api/v1/public/status-pages/by-domain/current",
            server.base_url
        ))
        .header("host", domain)
        .send()
        .await
        .expect("require private custom-domain access");
    assert_eq!(private_domain.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn status_page_logo_lifecycle_preserves_visibility_and_slug_rules() {
    if common::skip_unless_enabled() {
        return;
    }
    let server = common::TestServer::start().await;
    let (header_name, header_value) = server.auth_header();

    let create = server
        .client
        .post(format!("{}/api/v1/status-pages", server.base_url))
        .header(header_name, &header_value)
        .json(&page_settings("acme-customer", None, "public"))
        .send()
        .await
        .expect("create status page");
    assert_eq!(create.status(), StatusCode::CREATED);
    let created: Value = create.json().await.expect("created status page body");
    let page_id = created["id"].as_str().expect("page id");

    let invalid = server
        .client
        .post(format!(
            "{}/api/v1/status-pages/{page_id}/logo",
            server.base_url
        ))
        .header(header_name, &header_value)
        .header("content-type", "image/png")
        .body(b"<html>not a png</html>".to_vec())
        .send()
        .await
        .expect("reject invalid logo contents");
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let first_image = b"\x89PNG\r\n\x1a\nfirst-status-logo".to_vec();
    let first_upload = server
        .client
        .post(format!(
            "{}/api/v1/status-pages/{page_id}/logo",
            server.base_url
        ))
        .header(header_name, &header_value)
        .header("content-type", "image/png")
        .body(first_image.clone())
        .send()
        .await
        .expect("upload first logo");
    assert_eq!(first_upload.status(), StatusCode::OK);
    let first_page: Value = first_upload.json().await.expect("first upload response");
    let first_logo_url = first_page["logo_url"]
        .as_str()
        .expect("first logo url")
        .to_string();
    assert!(first_logo_url.starts_with("/api/v1/public/status-pages/acme-customer/logo/"));

    let public_first = server
        .client
        .get(format!("{}{}", server.base_url, first_logo_url))
        .send()
        .await
        .expect("serve first logo publicly");
    assert_eq!(public_first.status(), StatusCode::OK);
    assert_eq!(
        public_first
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok()),
        Some("image/png")
    );
    assert_eq!(
        public_first
            .headers()
            .get("x-content-type-options")
            .and_then(|value| value.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        public_first
            .bytes()
            .await
            .expect("first logo bytes")
            .as_ref(),
        first_image.as_slice()
    );

    let second_image = b"\x89PNG\r\n\x1a\nreplacement-status-logo".to_vec();
    let second_upload = server
        .client
        .post(format!(
            "{}/api/v1/status-pages/{page_id}/logo",
            server.base_url
        ))
        .header(header_name, &header_value)
        .header("content-type", "image/png")
        .body(second_image.clone())
        .send()
        .await
        .expect("replace status page logo");
    assert_eq!(second_upload.status(), StatusCode::OK);
    let second_page: Value = second_upload.json().await.expect("replacement response");
    let second_logo_url = second_page["logo_url"]
        .as_str()
        .expect("replacement logo url")
        .to_string();
    assert_ne!(second_logo_url, first_logo_url);

    let stale_logo = server
        .client
        .get(format!("{}{}", server.base_url, first_logo_url))
        .send()
        .await
        .expect("request stale logo");
    assert_eq!(stale_logo.status(), StatusCode::NOT_FOUND);

    let make_private = server
        .client
        .put(format!("{}/api/v1/status-pages/{page_id}", server.base_url))
        .header(header_name, &header_value)
        .json(&page_settings(
            "acme-customer",
            Some(&second_logo_url),
            "private",
        ))
        .send()
        .await
        .expect("make page private");
    assert_eq!(make_private.status(), StatusCode::OK);

    let private_branding_logo = server
        .client
        .get(format!("{}{}", server.base_url, second_logo_url))
        .send()
        .await
        .expect("request private page branding logo");
    assert_eq!(private_branding_logo.status(), StatusCode::OK);
    assert_eq!(
        private_branding_logo
            .bytes()
            .await
            .expect("private branding logo bytes")
            .as_ref(),
        second_image.as_slice()
    );

    let authenticated_preview = server
        .client
        .get(format!(
            "{}/api/v1/status-pages/{page_id}/logo",
            server.base_url
        ))
        .header(header_name, &header_value)
        .send()
        .await
        .expect("preview private page logo");
    assert_eq!(authenticated_preview.status(), StatusCode::OK);
    assert_eq!(
        authenticated_preview
            .bytes()
            .await
            .expect("private preview bytes")
            .as_ref(),
        second_image.as_slice()
    );

    let rename = server
        .client
        .put(format!("{}/api/v1/status-pages/{page_id}", server.base_url))
        .header(header_name, &header_value)
        .json(&page_settings(
            "acme-service-health",
            Some(&second_logo_url),
            "public",
        ))
        .send()
        .await
        .expect("rename status page");
    assert_eq!(rename.status(), StatusCode::OK);
    let renamed_page: Value = rename.json().await.expect("renamed page body");
    let renamed_logo_url = renamed_page["logo_url"]
        .as_str()
        .expect("renamed logo url")
        .to_string();
    assert!(renamed_logo_url.starts_with("/api/v1/public/status-pages/acme-service-health/logo/"));

    let renamed_logo = server
        .client
        .get(format!("{}{}", server.base_url, renamed_logo_url))
        .send()
        .await
        .expect("serve renamed page logo");
    assert_eq!(renamed_logo.status(), StatusCode::OK);

    let remove = server
        .client
        .put(format!("{}/api/v1/status-pages/{page_id}", server.base_url))
        .header(header_name, &header_value)
        .json(&page_settings("acme-service-health", None, "public"))
        .send()
        .await
        .expect("remove status page logo");
    assert_eq!(remove.status(), StatusCode::OK);
    let removed_page: Value = remove.json().await.expect("removed logo response");
    assert!(removed_page["logo_url"].is_null());

    let removed_preview = server
        .client
        .get(format!(
            "{}/api/v1/status-pages/{page_id}/logo",
            server.base_url
        ))
        .header(header_name, &header_value)
        .send()
        .await
        .expect("request removed logo preview");
    assert_eq!(removed_preview.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn subscriber_notifications_skip_hidden_components_and_carry_event_times() {
    if common::skip_unless_enabled() {
        return;
    }
    let server = common::TestServer::start().await;
    let (header_name, header_value) = server.auth_header();

    let create = server
        .client
        .post(format!("{}/api/v1/status-pages", server.base_url))
        .header(header_name, &header_value)
        .json(&page_settings("acme-notify", None, "public"))
        .send()
        .await
        .expect("create notification status page");
    assert_eq!(create.status(), StatusCode::CREATED);
    let created: Value = create.json().await.expect("created notification page body");
    let page_id = created["id"].as_str().expect("page id");

    let public_component = create_component(&server, page_id, "API", "enabled").await;
    let hidden_component = create_component(&server, page_id, "Billing worker", "hidden").await;

    let pool = sqlx::PgPool::connect(&server.settings.meta_database.dsn)
        .await
        .expect("connect status-page test pool");
    confirm_email_subscriber(&server, &pool, "acme-notify", page_id).await;

    set_component_status(
        &server,
        page_id,
        &hidden_component,
        "Billing worker",
        "hidden",
        "major_outage",
    )
    .await;
    set_component_status(
        &server,
        page_id,
        &public_component,
        "API",
        "enabled",
        "degraded_performance",
    )
    .await;

    let payloads = queued_payloads(&pool, page_id).await;
    assert_eq!(
        payloads.len(),
        1,
        "a hidden component must not notify subscribers: {payloads:?}"
    );
    assert_eq!(payloads[0]["type"], "component_status_changed");
    assert_eq!(payloads[0]["component_id"], public_component.as_str());
    assert_eq!(payloads[0]["component_name"], "API");
    assert_eq!(payloads[0]["status"], "degraded_performance");

    let planned_start =
        molesignal::shared::time::TimestampMicros::now().0 + 2 * 24 * 60 * 60 * 1_000_000_i64;
    let announce = server
        .client
        .post(format!(
            "{}/api/v1/status-pages/{page_id}/incidents",
            server.base_url
        ))
        .header(header_name, &header_value)
        .json(&json!({
            "kind": "maintenance",
            "title": "Database upgrade",
            "impact": "maintenance",
            "message": "Writes can take a few seconds longer.",
            "component_ids": [public_component, hidden_component],
            "started_at": planned_start
        }))
        .send()
        .await
        .expect("announce maintenance");
    assert_eq!(announce.status(), StatusCode::CREATED);
    let maintenance: Value = announce.json().await.expect("maintenance body");
    let maintenance_id = maintenance["id"].as_str().expect("maintenance id");

    let payloads = queued_payloads(&pool, page_id).await;
    assert_eq!(payloads.len(), 2);
    let announcement = &payloads[1];
    assert_eq!(announcement["type"], "incident_update");
    assert_eq!(announcement["incident_kind"], "maintenance");
    assert_eq!(announcement["status"], "scheduled");
    assert_eq!(announcement["started_at"], planned_start);
    assert_eq!(announcement.get("ended_at"), Some(&Value::Null));
    let published_at = announcement["occurred_at"].as_i64();
    assert!(published_at.expect("publication time") < planned_start);
    assert_eq!(
        announcement["component_names"],
        json!(["API"]),
        "a hidden component must not be named to subscribers"
    );

    let rescheduled_start = planned_start + 60 * 60 * 1_000_000_i64;
    let reschedule = server
        .client
        .put(format!(
            "{}/api/v1/status-pages/{page_id}/incidents/{maintenance_id}",
            server.base_url
        ))
        .header(header_name, &header_value)
        .json(&json!({
            "kind": "maintenance",
            "title": "Database upgrade",
            "impact": "maintenance",
            "message": "The upgrade starts one hour later.",
            "component_ids": [public_component, hidden_component],
            "started_at": rescheduled_start
        }))
        .send()
        .await
        .expect("reschedule maintenance");
    assert_eq!(reschedule.status(), StatusCode::OK);

    for (status, message) in [
        ("in_progress", "The upgrade has started."),
        ("completed", "The upgrade is complete."),
    ] {
        let update = server
            .client
            .post(format!(
                "{}/api/v1/status-pages/{page_id}/incidents/{maintenance_id}/updates",
                server.base_url
            ))
            .header(header_name, &header_value)
            .json(&json!({"status": status, "message": message}))
            .send()
            .await
            .expect("publish maintenance update");
        assert_eq!(update.status(), StatusCode::OK);
    }

    let payloads = queued_payloads(&pool, page_id).await;
    assert_eq!(payloads.len(), 5);
    let (rescheduled, started, completed) = (&payloads[2], &payloads[3], &payloads[4]);
    assert_eq!(rescheduled["status"], "scheduled");
    assert_eq!(rescheduled["started_at"], rescheduled_start);
    assert_eq!(started["status"], "in_progress");
    assert_eq!(started["started_at"], rescheduled_start);
    assert_eq!(started.get("ended_at"), Some(&Value::Null));
    assert_eq!(completed["status"], "completed");
    assert_eq!(completed["started_at"], rescheduled_start);
    assert!(completed["ended_at"].is_i64());
    assert_eq!(completed["ended_at"], completed["occurred_at"]);
    assert_eq!(completed["component_names"], json!(["API"]));
}

/// Subscribes an email address to a public page and confirms it.
///
/// The confirmation link only exists in an email that tests cannot read, and
/// without SMTP the request fails after it has stored the pending subscriber.
/// The confirmation is simulated on that stored subscriber.
async fn confirm_email_subscriber(
    server: &common::TestServer,
    pool: &sqlx::PgPool,
    slug: &str,
    page_id: &str,
) {
    server
        .client
        .post(format!(
            "{}/api/v1/public/status-pages/{slug}/subscriptions",
            server.base_url
        ))
        .json(&json!({"channel": "email", "target": "ops@customer.example"}))
        .send()
        .await
        .expect("request a status-page subscription");

    let confirmed = sqlx::query(
        "UPDATE status_page_subscribers
         SET status = 'active', confirmed_at_micros = $2, updated_at_micros = $2
         WHERE status_page_id = $1 AND status = 'pending'",
    )
    .bind(page_id)
    .bind(molesignal::shared::time::TimestampMicros::now().0)
    .execute(pool)
    .await
    .expect("confirm the pending subscriber")
    .rows_affected();
    assert_eq!(confirmed, 1);
}

async fn create_component(
    server: &common::TestServer,
    page_id: &str,
    name: &str,
    visibility: &str,
) -> String {
    let (header_name, header_value) = server.auth_header();

    let response = server
        .client
        .post(format!(
            "{}/api/v1/status-pages/{page_id}/components",
            server.base_url
        ))
        .header(header_name, &header_value)
        .json(&json!({"name": name, "status": "operational", "visibility": visibility}))
        .send()
        .await
        .expect("create status component");
    assert_eq!(response.status(), StatusCode::CREATED);

    let component: Value = response.json().await.expect("component body");
    component["id"].as_str().expect("component id").to_string()
}

async fn set_component_status(
    server: &common::TestServer,
    page_id: &str,
    component_id: &str,
    name: &str,
    visibility: &str,
    status: &str,
) {
    let (header_name, header_value) = server.auth_header();

    let response = server
        .client
        .put(format!(
            "{}/api/v1/status-pages/{page_id}/components/{component_id}",
            server.base_url
        ))
        .header(header_name, &header_value)
        .json(&json!({"name": name, "status": status, "visibility": visibility}))
        .send()
        .await
        .expect("update status component");
    assert_eq!(response.status(), StatusCode::OK);
}

/// Payloads waiting for the subscribers of a page, oldest first.
async fn queued_payloads(pool: &sqlx::PgPool, page_id: &str) -> Vec<Value> {
    sqlx::query_scalar::<Value>(
        "SELECT payload FROM status_page_notification_deliveries
         WHERE status_page_id = $1 ORDER BY created_at_micros, id",
    )
    .bind(page_id)
    .fetch_all(pool)
    .await
    .expect("read queued status-page notifications")
}

#[tokio::test]
async fn backend_language_table_serves_the_page_language_and_falls_back_to_english() {
    use molesignal::{
        domain::i18n_language::I18nLanguageRepository,
        infra::persistence::repositories::i18n_language::PgI18nLanguageRepository,
    };

    if common::skip_unless_enabled() {
        return;
    }
    let server = common::TestServer::start().await;
    let pool = sqlx::PgPool::connect(&server.settings.meta_database.dsn)
        .await
        .expect("connect status-page test pool");
    let repository = PgI18nLanguageRepository::new(pool.clone());
    let operational = "status_page.component_status.operational";

    let chinese = repository
        .load_pack("status_page.", "zh-cn")
        .await
        .expect("load the Chinese pack");
    assert_eq!(chinese.get(operational), Some("正常"));
    assert_eq!(
        chinese
            .render(
                "status_page.email.maintenance_update.body",
                &[
                    ("title", "数据库升级"),
                    ("status", "已计划"),
                    (
                        "scheduled_for",
                        "2026-05-25 18:00 UTC+08:00 (Asia/Shanghai)"
                    ),
                    ("message", "写入可能延迟数秒。"),
                    ("page_url", "https://status.acme.example/"),
                ],
            )
            .expect("write the maintenance email"),
        "数据库升级\n已计划\n计划时间：2026-05-25 18:00 UTC+08:00 (Asia/Shanghai)\n\n写入可能延迟数秒。\n\nhttps://status.acme.example/"
    );

    let unseeded = repository
        .load_pack("status_page.", "fr-fr")
        .await
        .expect("load a language without entries");
    assert_eq!(unseeded.get(operational), Some("Operational"));

    sqlx::query("DELETE FROM i18n_language WHERE language_key = $1 AND language_code = 'zh-cn'")
        .bind(operational)
        .execute(&pool)
        .await
        .expect("remove one Chinese entry");
    let partial = repository
        .load_pack("status_page.", "zh-cn")
        .await
        .expect("load the Chinese pack again");
    assert_eq!(partial.get(operational), Some("Operational"));
    assert_eq!(
        partial.get("status_page.component_status.maintenance"),
        Some("维护中")
    );

    let emails = repository
        .load_pack("status_page.email.", "en-us")
        .await
        .expect("load the email entries alone");
    assert_eq!(emails.get(operational), None);
    assert_eq!(
        emails.get("status_page.email.access_link.subject"),
        Some("Access {page_name}")
    );
}

#[tokio::test]
async fn planned_maintenance_reminds_subscribers_and_starts_at_its_planned_time() {
    const MINUTE: i64 = 60 * 1_000_000;

    if common::skip_unless_enabled() {
        return;
    }
    let server = common::TestServer::start().await;
    let (header_name, header_value) = server.auth_header();

    // A Chinese page, so that what the page writes by itself is in Chinese.
    let mut settings = page_settings("acme-planned", None, "public");
    settings["language"] = json!("zh-cn");
    settings["languages"] = json!(["zh-cn", "en-us"]);
    let create = server
        .client
        .post(format!("{}/api/v1/status-pages", server.base_url))
        .header(header_name, &header_value)
        .json(&settings)
        .send()
        .await
        .expect("create planned-maintenance status page");
    assert_eq!(create.status(), StatusCode::CREATED);
    let created: Value = create.json().await.expect("created page body");
    let page_id = created["id"].as_str().expect("page id");

    let component = create_component(&server, page_id, "API", "enabled").await;
    let pool = sqlx::PgPool::connect(&server.settings.meta_database.dsn)
        .await
        .expect("connect status-page test pool");
    confirm_email_subscriber(&server, &pool, "acme-planned", page_id).await;

    let announced_at = molesignal::shared::time::TimestampMicros::now().0;
    let announce = |title: &'static str, options: Value| {
        let mut body = json!({
            "kind": "maintenance",
            "title": title,
            "impact": "maintenance",
            "message": "写入可能延迟数秒。",
            "component_ids": [component],
            "started_at": announced_at + 180 * MINUTE
        });
        body.as_object_mut()
            .expect("request object")
            .extend(options.as_object().expect("options object").clone());

        server
            .client
            .post(format!(
                "{}/api/v1/status-pages/{page_id}/incidents",
                server.base_url
            ))
            .header(header_name, &header_value)
            .json(&body)
            .send()
    };

    // Maintenance that names neither option starts by itself and reminds an hour ahead.
    let automatic = announce("数据库升级", json!({}))
        .await
        .expect("announce maintenance");
    assert_eq!(automatic.status(), StatusCode::CREATED);
    let automatic: Value = automatic.json().await.expect("maintenance body");
    let automatic_id = automatic["id"].as_str().expect("maintenance id");
    assert_eq!(automatic["auto_start"], true);
    assert_eq!(automatic["reminder_minutes"], 60);

    let manual = announce(
        "网络设备更换",
        json!({"auto_start": false, "reminder_minutes": 0}),
    )
    .await
    .expect("announce manual maintenance");
    assert_eq!(manual.status(), StatusCode::CREATED);
    let manual: Value = manual.json().await.expect("manual maintenance body");
    let manual_id = manual["id"].as_str().expect("manual maintenance id");
    assert_eq!(manual["auto_start"], false);
    assert_eq!(manual["reminder_minutes"], 0);

    // Nothing is due three hours ahead.
    server
        .state
        .status_pages
        .run_planned_maintenance(100)
        .await
        .expect("sweep before anything is due");
    assert!(reminders(&pool, page_id).await.is_empty());

    // Two hours pass: both were announced two hours ago and start in 30 minutes.
    let soon = molesignal::shared::time::TimestampMicros::now().0 + 30 * MINUTE;
    sqlx::query(
        "UPDATE status_page_incident_updates SET created_at_micros = created_at_micros - $2
         WHERE status_page_id = $1",
    )
    .bind(page_id)
    .bind(120 * MINUTE)
    .execute(&pool)
    .await
    .expect("age the announcements");
    set_planned_start(&pool, page_id, soon).await;

    // The background worker may sweep too; either way the reminder exists once.
    for _ in 0..2 {
        server
            .state
            .status_pages
            .run_planned_maintenance(100)
            .await
            .expect("sweep inside the reminder window");
    }
    let sent = reminders(&pool, page_id).await;
    assert_eq!(sent.len(), 1, "one reminder for one subscriber: {sent:?}");
    assert_eq!(sent[0]["incident_id"], automatic_id);
    assert_eq!(sent[0]["title"], "数据库升级");
    assert_eq!(sent[0]["started_at"], soon);
    assert_eq!(sent[0]["component_names"], json!(["API"]));
    assert!(sent[0]["occurred_at"].as_i64().expect("reminder time") < soon);

    // The planned start arrives.
    let due = molesignal::shared::time::TimestampMicros::now().0 - 1_000_000;
    set_planned_start(&pool, page_id, due).await;
    for _ in 0..2 {
        server
            .state
            .status_pages
            .run_planned_maintenance(100)
            .await
            .expect("sweep at the planned start");
    }

    let started = maintenance(&server, page_id, automatic_id).await;
    assert_eq!(started["status"], "in_progress");
    let automatic_updates: Vec<&Value> = started["updates"]
        .as_array()
        .expect("updates")
        .iter()
        .filter(|update| update["status"] == "in_progress")
        .collect();
    assert_eq!(
        automatic_updates.len(),
        1,
        "the automatic update is published once"
    );
    assert_eq!(automatic_updates[0]["message"], "维护已按计划开始。");

    let notified: Vec<Value> = queued_payloads(&pool, page_id)
        .await
        .into_iter()
        .filter(|payload| {
            payload["type"] == "incident_update" && payload["status"] == "in_progress"
        })
        .collect();
    assert_eq!(notified.len(), 1);
    assert_eq!(notified[0]["incident_id"], automatic_id);
    assert_eq!(notified[0]["message"], "维护已按计划开始。");
    assert_eq!(notified[0]["started_at"], due);

    // Maintenance that opted out waits for a person, and an edit that names
    // neither option keeps them off.
    assert_eq!(
        maintenance(&server, page_id, manual_id).await["status"],
        "scheduled"
    );
    assert_eq!(reminders(&pool, page_id).await.len(), 1);
    let reschedule = server
        .client
        .put(format!(
            "{}/api/v1/status-pages/{page_id}/incidents/{manual_id}",
            server.base_url
        ))
        .header(header_name, &header_value)
        .json(&json!({
            "kind": "maintenance",
            "title": "网络设备更换",
            "impact": "maintenance",
            "message": "改到明天进行。",
            "component_ids": [component],
            "started_at": due + 24 * 60 * MINUTE
        }))
        .send()
        .await
        .expect("reschedule manual maintenance");
    assert_eq!(reschedule.status(), StatusCode::OK);
    let rescheduled: Value = reschedule.json().await.expect("rescheduled body");
    assert_eq!(rescheduled["auto_start"], false);
    assert_eq!(rescheduled["reminder_minutes"], 0);
}

/// Moves the planned start of every scheduled maintenance of a page, the way time passing would.
async fn set_planned_start(pool: &sqlx::PgPool, page_id: &str, planned_start: i64) {
    sqlx::query(
        "UPDATE status_page_incidents SET started_at_micros = $2
         WHERE status_page_id = $1 AND kind = 'maintenance' AND status = 'scheduled'",
    )
    .bind(page_id)
    .bind(planned_start)
    .execute(pool)
    .await
    .expect("move the planned start");
}

async fn reminders(pool: &sqlx::PgPool, page_id: &str) -> Vec<Value> {
    queued_payloads(pool, page_id)
        .await
        .into_iter()
        .filter(|payload| payload["type"] == "maintenance_reminder")
        .collect()
}

async fn maintenance(server: &common::TestServer, page_id: &str, maintenance_id: &str) -> Value {
    let (header_name, header_value) = server.auth_header();

    let response = server
        .client
        .get(format!(
            "{}/api/v1/status-pages/{page_id}/incidents/{maintenance_id}",
            server.base_url
        ))
        .header(header_name, &header_value)
        .send()
        .await
        .expect("read maintenance");
    assert_eq!(response.status(), StatusCode::OK);

    response.json().await.expect("maintenance body")
}
