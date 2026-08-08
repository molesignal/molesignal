// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Standalone distributed-tracing acceptance path. The test is environment-gated because it
//! starts PostgreSQL through testcontainers.

mod common;

use std::sync::Arc;

use axum::{Router, extract::State, http::StatusCode, middleware::from_fn, routing::get};
use base64::Engine as _;
use common::{TestServer, skip_unless_enabled, wait_until_async};
use molesignal::{
    domain::license::{LicenseVersion, LicenseVersionRepository},
    infra::persistence::repositories::audit_events::AuditEvent,
    shared::time::TimestampMicros,
};
use object_store::{ObjectStore, ObjectStoreExt, PutPayload, path::Path as ObjectPath};
use serde_json::{Value, json};
use sqlx::postgres::PgPoolOptions;
use tracing::Instrument;

#[derive(Clone)]
struct ProbeState {
    pool: sqlx::PgPool,
    object_store: Arc<dyn ObjectStore>,
}

async fn traced_business_probe(
    State(state): State<ProbeState>,
) -> Result<StatusCode, (StatusCode, String)> {
    let span = tracing::info_span!(
        target: "molesignal::it::distributed_tracing",
        "business.trace_e2e",
        otel.kind = "internal",
        molesignal.business.operation = "trace_e2e_probe",
        authorization = "Bearer trace-e2e-forbidden-token",
        user.email = "trace-e2e-private@example.invalid",
    );
    async move {
        let value = sqlx::query_scalar::<i64>("SELECT 1::BIGINT")
            .fetch_one(&state.pool)
            .await
            .map_err(internal_error)?;
        state
            .object_store
            .put(
                &ObjectPath::from("trace-e2e/probe.parquet"),
                PutPayload::from_static(b"trace-e2e"),
            )
            .await
            .map_err(internal_error)?;
        tracing::info!(
            target: "molesignal::it::distributed_tracing",
            marker = "trace_e2e",
            db.value = value,
            authorization = "Bearer trace-e2e-forbidden-token",
            user.email = "trace-e2e-private@example.invalid",
            "trace correlation probe completed"
        );
        Ok(StatusCode::NO_CONTENT)
    }
    .instrument(span)
    .await
}

fn internal_error(error: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

async fn query_system_stream(
    client: &reqwest::Client,
    base_url: &str,
    system_token: &str,
    system_org_id: &str,
    stream_type: &str,
    statement: &str,
) -> Option<Value> {
    let response = client
        .post(format!("{base_url}/api/v1/query"))
        .bearer_auth(system_token)
        .json(&json!({
            "org_id": system_org_id,
            "language": "sql",
            "statement": statement,
            "time_range": {
                "start": 0,
                "end": TimestampMicros::now().0 + 60_000_000
            },
            "stream": {
                "name": "_molesignal",
                "stream_type": stream_type
            }
        }))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response.json().await.ok()
}

fn first_column_strings(result: &Value) -> Vec<&str> {
    result["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row.as_array()?.first()?.as_str())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn http_sql_object_store_business_spans_and_logs_are_queryable_in_system_scope() {
    if skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }

    let server = TestServer::start_with_trace_capture().await;
    let api_token_response: Value = server
        .client
        .post(format!("{}/api/v1/auth/tokens", server.base_url))
        .bearer_auth(&server.root_token)
        .json(&json!({"name": "trace-system-boundary", "role": "owner"}))
        .send()
        .await
        .expect("create tenant API token")
        .error_for_status()
        .expect("tenant API token creation succeeds")
        .json()
        .await
        .expect("tenant API token JSON");
    let tenant_api_token = api_token_response["token"]
        .as_str()
        .expect("tenant API token plaintext")
        .to_owned();
    for path in [
        "/api/v1/system/telemetry",
        "/api/v1/system/license",
        "/api/v1/system/platform-admins",
        "/api/v1/license",
    ] {
        let response = server
            .client
            .get(format!("{}{path}", server.base_url))
            .bearer_auth(&server.root_token)
            .send()
            .await
            .expect("tenant-scoped system API request");
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "tenant scope must not discover {path}"
        );
    }
    let tenant_orgs: Value = server
        .client
        .get(format!("{}/api/v1/orgs", server.base_url))
        .bearer_auth(&server.root_token)
        .send()
        .await
        .expect("list tenant-visible organizations")
        .json()
        .await
        .expect("tenant organization JSON");
    assert!(
        tenant_orgs
            .as_array()
            .is_some_and(|organizations| organizations.iter().all(|org| {
                org["id"].as_str() != Some(server.state.iam.system_org_id.as_str())
                    && org["system"].as_bool() == Some(false)
            })),
        "ordinary users must not discover `_sys`"
    );
    let select_url = format!(
        "{}/api/v1/orgs/{}/select",
        server.base_url,
        server.state.iam.system_org_id.as_str()
    );
    let response = server
        .client
        .post(&select_url)
        .bearer_auth(&server.root_token)
        .send()
        .await
        .expect("unauthorized system selection");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    server
        .state
        .iam
        .platform_administrators
        .bootstrap_root(&server.root_user_id)
        .await
        .expect("reconcile root platform administrator");
    let visible_orgs: Value = server
        .client
        .get(format!("{}/api/v1/orgs", server.base_url))
        .bearer_auth(&server.root_token)
        .send()
        .await
        .expect("list platform-visible organizations")
        .json()
        .await
        .expect("platform organization JSON");
    assert!(
        visible_orgs.as_array().is_some_and(|organizations| {
            organizations.iter().any(|org| {
                org["id"].as_str() == Some(server.state.iam.system_org_id.as_str())
                    && org["system"].as_bool() == Some(true)
                    && org["role"].is_null()
            })
        }),
        "platform administrators must discover `_sys` without IamMembership"
    );
    for path in [
        "/api/v1/system/telemetry",
        "/api/v1/system/license",
        "/api/v1/system/platform-admins",
    ] {
        let response = server
            .client
            .get(format!("{}{path}", server.base_url))
            .bearer_auth(&server.root_token)
            .send()
            .await
            .expect("tenant-scoped platform-administrator request");
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "a platform administrator must select `_sys` before accessing {path}"
        );
    }
    for path in [
        "/api/v1/system/telemetry",
        "/api/v1/system/license",
        "/api/v1/system/platform-admins",
    ] {
        let response = server
            .client
            .get(format!("{}{path}", server.base_url))
            .bearer_auth(&tenant_api_token)
            .send()
            .await
            .expect("tenant API-token system request");
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "ms_* API tokens must never access {path}"
        );
    }
    let api_token_select = server
        .client
        .post(&select_url)
        .bearer_auth(&tenant_api_token)
        .send()
        .await
        .expect("API-token system selection");
    assert_eq!(api_token_select.status(), StatusCode::NOT_FOUND);
    let selection: Value = server
        .client
        .post(&select_url)
        .bearer_auth(&server.root_token)
        .send()
        .await
        .expect("select system organization")
        .error_for_status()
        .expect("platform administrator can select `_sys`")
        .json()
        .await
        .expect("system selection JSON");
    assert_eq!(
        selection["org_id"].as_str(),
        Some(server.state.iam.system_org_id.as_str())
    );
    assert_eq!(selection["system"].as_bool(), Some(true));
    assert_eq!(selection["display_role"].as_str(), Some("Owner"));
    assert_eq!(
        selection["roles"][0]["key"].as_str(),
        Some("platform_owner")
    );
    assert!(
        selection.get("role").is_none(),
        "system selection must not expose the removed compatibility role field"
    );
    let system_token = selection["token"]
        .as_str()
        .expect("system-scope JWT")
        .to_owned();
    let claims: Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(
                system_token
                    .split('.')
                    .nth(1)
                    .expect("system-scope JWT payload"),
            )
            .expect("decode system-scope JWT payload"),
    )
    .expect("system-scope JWT claims");
    assert_eq!(claims["scope"].as_str(), Some("system"));
    assert!(
        claims.get("role").is_none(),
        "system JWT must not carry a role"
    );
    assert!(
        claims.get("platform_permissions").is_none(),
        "system JWT must not carry platform permissions"
    );
    assert!(
        claims["exp"]
            .as_u64()
            .zip(claims["iat"].as_u64())
            .is_some_and(|(expires, issued)| expires.saturating_sub(issued) <= 3_600),
        "system-scope JWT lifetime must be at most one hour"
    );
    assert!(
        server
            .state
            .iam
            .service
            .iam_memberships
            .list_for_user(&server.root_user_id)
            .await
            .expect("list tenant memberships")
            .iter()
            .all(|membership| membership.org_id != server.state.iam.system_org_id),
        "system-scope access must not depend on `_sys` IamMembership"
    );
    let system_scoped_orgs: Value = server
        .client
        .get(format!("{}/api/v1/orgs", server.base_url))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("list organizations from system scope")
        .error_for_status()
        .expect("system scope organization list succeeds")
        .json()
        .await
        .expect("system scope organization JSON");
    assert!(
        system_scoped_orgs.as_array().is_some_and(|organizations| {
            organizations.iter().any(|org| {
                org["id"].as_str() == Some(server.state.iam.system_org_id.as_str())
                    && org["name"].as_str() == Some("_sys")
                    && org["system"].as_bool() == Some(true)
            }) && organizations.iter().any(|org| {
                org["id"].as_str() == Some(server.root_org_id.as_str())
                    && org["system"].as_bool() == Some(false)
            })
        }),
        "system scope must list both `_sys` and the user's tenant organizations"
    );
    let tenant_selection: Value = server
        .client
        .post(format!(
            "{}/api/v1/orgs/{}/select",
            server.base_url,
            server.root_org_id.as_str()
        ))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("switch from system scope back to tenant")
        .error_for_status()
        .expect("system scope can select a tenant membership")
        .json()
        .await
        .expect("tenant selection JSON");
    assert_eq!(
        tenant_selection["org_id"].as_str(),
        Some(server.root_org_id.as_str())
    );
    assert_eq!(tenant_selection["org_name"].as_str(), Some("default"));
    assert_eq!(tenant_selection["system"].as_bool(), Some(false));
    for path in [
        "/api/v1/system/telemetry",
        "/api/v1/system/license",
        "/api/v1/system/platform-admins",
    ] {
        let response = server
            .client
            .get(format!("{}{path}", server.base_url))
            .bearer_auth(&system_token)
            .send()
            .await
            .expect("system-scoped API request");
        assert!(
            response.status().is_success(),
            "system scope should access {path}, got {}",
            response.status()
        );
    }
    let forbidden_stream_create = server
        .client
        .post(format!("{}/api/v1/streams", server.base_url))
        .bearer_auth(&system_token)
        .json(&json!({
            "name": "system-owner-must-not-write",
            "stream_type": "logs",
            "fields": [],
        }))
        .send()
        .await
        .expect("system-scoped stream mutation request");
    assert_eq!(
        forbidden_stream_create.status(),
        StatusCode::FORBIDDEN,
        "the Owner display role must not grant ordinary stream mutation"
    );
    let hidden_api_token_create = server
        .client
        .post(format!("{}/api/v1/auth/tokens", server.base_url))
        .bearer_auth(&system_token)
        .json(&json!({"name": "system-owner-must-not-create-token"}))
        .send()
        .await
        .expect("system-scoped API-token creation request");
    assert_eq!(
        hidden_api_token_create.status(),
        StatusCode::NOT_FOUND,
        "the Owner display role must not create an API token for `_sys`"
    );
    let telemetry: Value = server
        .client
        .get(format!("{}/api/v1/system/telemetry", server.base_url))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("read Trace policy before privacy update")
        .error_for_status()
        .expect("read Trace policy succeeds")
        .json()
        .await
        .expect("Trace telemetry JSON");
    let mut policy = telemetry["policy"].clone();
    policy.as_object_mut().expect("Trace policy object").insert(
        "authorization".into(),
        json!("Bearer trace-config-forbidden-token"),
    );
    server
        .client
        .put(format!("{}/api/v1/system/telemetry", server.base_url))
        .bearer_auth(&system_token)
        .json(&policy)
        .send()
        .await
        .expect("update Trace policy with ignored hostile field")
        .error_for_status()
        .expect("Trace policy update succeeds");
    let removed_admin_mutation = server
        .client
        .delete(format!(
            "{}/api/v1/system/platform-admins/{}",
            server.base_url,
            server.root_user_id.as_str()
        ))
        .bearer_auth(&system_token)
        .send()
        .await
        .expect("request removed platform-administrator mutation endpoint");
    assert_eq!(removed_admin_mutation.status(), StatusCode::NOT_FOUND);

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&server.settings.store.meta.dsn)
        .await
        .expect("connect trace probe pool");
    let first_org_repo =
        molesignal::infra::persistence::repositories::organizations::PgOrganizationRepository::new(
            pool.clone(),
        );
    let second_org_repo =
        molesignal::infra::persistence::repositories::organizations::PgOrganizationRepository::new(
            pool.clone(),
        );
    let (first_system_org, second_system_org) = tokio::join!(
        first_org_repo.ensure_system_organization(),
        second_org_repo.ensure_system_organization()
    );
    assert_eq!(
        first_system_org
            .expect("first concurrent `_sys` bootstrap")
            .id,
        second_system_org
            .expect("second concurrent `_sys` bootstrap")
            .id
    );
    let system_org_count =
        sqlx::query_scalar::<i64>("SELECT COUNT(*)::BIGINT FROM organizations WHERE system")
            .fetch_one(&pool)
            .await
            .expect("count system organizations");
    assert_eq!(system_org_count, 1);
    assert!(
        sqlx::query("UPDATE organizations SET name = 'tampered' WHERE id = $1")
            .bind(server.state.iam.system_org_id.as_str())
            .execute(&pool)
            .await
            .is_err(),
        "database must reject `_sys` identity changes"
    );
    assert!(
        sqlx::query("DELETE FROM organizations WHERE id = $1")
            .bind(server.state.iam.system_org_id.as_str())
            .execute(&pool)
            .await
            .is_err(),
        "database must reject `_sys` deletion"
    );
    assert!(
        sqlx::query(
            "INSERT INTO iam_memberships (user_id, org_id, joined_at_micros)
             VALUES ($1, $2, $3)",
        )
        .bind(server.root_user_id.as_str())
        .bind(server.state.iam.system_org_id.as_str())
        .bind(TimestampMicros::now().0)
        .execute(&pool)
        .await
        .is_err(),
        "database must reject `_sys` IamMembership"
    );

    let trace_stream_id = sqlx::query_scalar::<String>(
        "SELECT id FROM streams
         WHERE org_id = $1 AND name = '_molesignal' AND stream_type = 'traces' AND system",
    )
    .bind(server.state.iam.system_org_id.as_str())
    .fetch_one(&pool)
    .await
    .expect("protected system Trace stream");
    sqlx::query(
        "UPDATE streams
         SET retention = '{\"days\": 8}'::jsonb, updated_at_micros = $2
         WHERE id = $1",
    )
    .bind(&trace_stream_id)
    .bind(TimestampMicros::now().0)
    .execute(&pool)
    .await
    .expect("retention is an approved system-stream capacity mutation");
    for statement in [
        "UPDATE streams SET name = 'tampered' WHERE id = $1",
        "UPDATE streams SET system = FALSE WHERE id = $1",
        "UPDATE streams SET schema = '{\"fields\": []}'::jsonb WHERE id = $1",
        "DELETE FROM streams WHERE id = $1",
    ] {
        assert!(
            sqlx::query(statement)
                .bind(&trace_stream_id)
                .execute(&pool)
                .await
                .is_err(),
            "database accepted forbidden system-stream mutation: {statement}"
        );
    }

    let license_versions = molesignal::infra::persistence::repositories::license_versions::
        PgLicenseVersionRepository::new(pool.clone());
    let first_license_id = molesignal::shared::ids::Id::new();
    let first_digest = format!("trace-e2e-{}", first_license_id.as_str());
    license_versions
        .insert_and_activate(
            LicenseVersion {
                id: first_license_id.clone(),
                system_org_id: server.state.iam.system_org_id.clone(),
                signed_package: json!({
                    "payload_b64": "fixture-a",
                    "signature_b64": "fixture-a"
                }),
                payload_digest: first_digest.clone(),
                summary: json!({"fixture": "a"}),
                created_by: Some(server.root_user_id.clone()),
                created_at: TimestampMicros::now(),
            },
            Some(&server.root_user_id),
        )
        .await
        .expect("atomically insert and activate first License version");
    let second_license_id = molesignal::shared::ids::Id::new();
    license_versions
        .insert_and_activate(
            LicenseVersion {
                id: second_license_id.clone(),
                system_org_id: server.state.iam.system_org_id.clone(),
                signed_package: json!({
                    "payload_b64": "fixture-b",
                    "signature_b64": "fixture-b"
                }),
                payload_digest: format!("trace-e2e-{}", second_license_id.as_str()),
                summary: json!({"fixture": "b"}),
                created_by: Some(server.root_user_id.clone()),
                created_at: TimestampMicros::now(),
            },
            Some(&server.root_user_id),
        )
        .await
        .expect("atomically insert and activate second License version");
    license_versions
        .activate(&first_license_id, &server.root_user_id)
        .await
        .expect("reactivate historical License version");
    assert_eq!(
        license_versions
            .active()
            .await
            .expect("read active License")
            .expect("active License pointer")
            .version
            .id,
        first_license_id
    );

    let conflicting_license_id = molesignal::shared::ids::Id::new();
    assert!(
        license_versions
            .insert_and_activate(
                LicenseVersion {
                    id: conflicting_license_id.clone(),
                    system_org_id: server.state.iam.system_org_id.clone(),
                    signed_package: json!({
                        "payload_b64": "conflict",
                        "signature_b64": "conflict"
                    }),
                    payload_digest: first_digest,
                    summary: json!({"fixture": "conflict"}),
                    created_by: Some(server.root_user_id.clone()),
                    created_at: TimestampMicros::now(),
                },
                Some(&server.root_user_id),
            )
            .await
            .is_err(),
        "failed history insert must roll back active-pointer mutation"
    );
    assert!(
        license_versions.get(&conflicting_license_id).await.is_err(),
        "failed License transaction must not leave a history row"
    );
    assert_eq!(
        license_versions
            .active()
            .await
            .expect("read active License after rollback")
            .expect("active License pointer after rollback")
            .version
            .id,
        first_license_id,
        "failed License transaction must not change the active pointer"
    );
    let reconnected_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&server.settings.store.meta.dsn)
        .await
        .expect("reconnect License repository");
    let reconnected_versions = molesignal::infra::persistence::repositories::license_versions::
        PgLicenseVersionRepository::new(reconnected_pool);
    assert_eq!(
        reconnected_versions
            .active()
            .await
            .expect("reload persisted active License")
            .expect("persisted active License pointer")
            .version
            .id,
        first_license_id,
        "active License must persist across repository/process reconnection"
    );
    for statement in [
        "UPDATE license_versions SET summary = '{\"tampered\": true}'::jsonb WHERE id = $1",
        "DELETE FROM license_versions WHERE id = $1",
    ] {
        assert!(
            sqlx::query(statement)
                .bind(first_license_id.as_str())
                .execute(&pool)
                .await
                .is_err(),
            "database accepted immutable License history mutation: {statement}"
        );
    }
    assert!(
        sqlx::query("UPDATE iam_platform_administrators SET active = FALSE WHERE user_id = $1")
            .bind(server.root_user_id.as_str())
            .execute(&pool)
            .await
            .is_err(),
        "database must protect the configured root assignment"
    );
    assert!(
        sqlx::query("UPDATE users SET disabled = TRUE WHERE id = $1")
            .bind(server.root_user_id.as_str())
            .execute(&pool)
            .await
            .is_err(),
        "database must keep the configured root user active"
    );

    let probe_app = Router::new()
        .route("/trace-e2e", get(traced_business_probe))
        .layer(from_fn(
            molesignal::api::http::middleware::trace_context_layer,
        ))
        .with_state(ProbeState {
            pool,
            object_store: server.state.storage.object_store.clone(),
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind trace probe");
    let probe_url = format!(
        "http://{}/trace-e2e",
        listener.local_addr().expect("trace probe local address")
    );
    let probe_server = tokio::spawn(async move {
        let _ = axum::serve(listener, probe_app).await;
    });

    let response = server
        .client
        .get(probe_url)
        .header("x-request-id", "trace-e2e-request")
        .send()
        .await
        .expect("call traced business probe");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response
            .headers()
            .get("x-request-id")
            .and_then(|value| value.to_str().ok()),
        Some("trace-e2e-request")
    );
    let trace_id = response
        .headers()
        .get("x-trace-id")
        .and_then(|value| value.to_str().ok())
        .expect("X-Trace-Id response header")
        .to_owned();
    assert_eq!(trace_id.len(), 32);

    let trace_statement = format!(
        "SELECT name FROM _molesignal WHERE trace_id = '{trace_id}' \
         ORDER BY start_time_unix_nano"
    );
    let expected_names = [
        "http.server",
        "business.trace_e2e",
        "db.query",
        "object_store.operation",
    ];
    let trace_ready = wait_until_async(30, {
        let client = server.client.clone();
        let base_url = server.base_url.clone();
        let token = system_token.clone();
        let system_org_id = server.state.iam.system_org_id.0.clone();
        let statement = trace_statement.clone();
        move || {
            let client = client.clone();
            let base_url = base_url.clone();
            let token = token.clone();
            let system_org_id = system_org_id.clone();
            let statement = statement.clone();
            async move {
                query_system_stream(
                    &client,
                    &base_url,
                    &token,
                    &system_org_id,
                    "traces",
                    &statement,
                )
                .await
                .is_some_and(|result| {
                    let names = first_column_strings(&result);
                    expected_names.iter().all(|name| names.contains(name))
                })
            }
        }
    })
    .await;
    assert!(
        trace_ready,
        "HTTP/SQL/object-store/business spans were not queryable from `_sys/traces/_molesignal`"
    );

    let result = query_system_stream(
        &server.client,
        &server.base_url,
        &system_token,
        server.state.iam.system_org_id.as_str(),
        "traces",
        &format!("SELECT * FROM _molesignal WHERE trace_id = '{trace_id}'"),
    )
    .await
    .expect("query privacy rows from system traces stream");
    let encoded = serde_json::to_string(&result).expect("encode system telemetry query");
    assert!(!encoded.contains("trace-e2e-forbidden-token"));
    assert!(!encoded.contains("trace-e2e-private@example.invalid"));

    server
        .state
        .iam
        .audit_events
        .record(AuditEvent {
            id: molesignal::shared::ids::Id::new(),
            org_id: server.state.iam.system_org_id.clone(),
            actor_kind: "user".into(),
            actor_id: server.root_user_id.0.clone(),
            action: "trace_privacy.probe".into(),
            target_kind: Some("trace".into()),
            target_id: Some(trace_id.clone()),
            ip: None,
            user_agent: Some("Bearer trace-audit-forbidden-token".into()),
            payload: json!({
                "authorization": "Bearer trace-audit-forbidden-token",
                "nested": {"email": "trace-audit-private@example.invalid"},
                "safe": true
            }),
            ts: TimestampMicros::now(),
        })
        .await
        .expect("persist privacy-probe audit event");
    let audit = server
        .state
        .iam
        .audit_events
        .list_recent(&server.state.iam.system_org_id, 100)
        .await
        .expect("read system audit events");
    assert!(
        audit
            .iter()
            .any(|event| event.action == "trace_privacy.probe"),
        "privacy-probe audit event must persist"
    );
    let encoded_audit = serde_json::to_string(&audit).expect("encode system audit events");
    for forbidden in [
        "trace-config-forbidden-token",
        "trace-audit-forbidden-token",
        "trace-audit-private@example.invalid",
    ] {
        assert!(
            !encoded_audit.contains(forbidden),
            "system audit/config diff leaked `{forbidden}`"
        );
    }

    probe_server.abort();
    if let Some(runtime) = &server.state.telemetry.self_telemetry_runtime {
        runtime.stop_and_flush().await;
    }
    server.state.telemetry.trace_candidates.shutdown().await;
    server.state.telemetry.trace_pipeline.shutdown().await;
}
