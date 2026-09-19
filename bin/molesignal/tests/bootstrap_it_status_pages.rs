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
    let pool = sqlx::PgPool::connect(&server.settings.store.meta.dsn)
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
