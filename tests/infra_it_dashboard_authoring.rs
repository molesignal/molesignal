// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use molesignal::{
    app::dashboard::{
        authoring::DashboardAuthoringCompiler,
        contract_registry::{
            DashboardContractRegistryService, DashboardContractResolver, ResolvedDashboardContracts,
        },
        validation::{DashboardValidationMode, validate_dashboard_model},
    },
    config::MetaStoreSettings,
    domain::dashboard::{
        Dashboard,
        authoring::{
            ConsumeDashboardDraft, DashboardAuthoringSpec, DashboardDraft,
            DashboardDraftRepository, DashboardDraftStatus, DraftConsumption, PreflightReport,
        },
        contract_registry::DASHBOARD_AUTHORING_CAPABILITY,
    },
    infra::persistence::{
        MetaStore,
        repositories::{
            dashboard_authoring::PgDashboardDraftRepository,
            dashboard_contract_registry::PgDashboardContractRepository,
        },
    },
    shared::{Error, ids::Id, time::TimestampMicros},
};
use sqlx::Row;

const VALID_AUTHORING: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/dashboard/fixtures/valid/authoring-v1-promql.json"
));

fn skip_unless_enabled() -> bool {
    std::env::var("MS_RUN_IT").ok().as_deref() != Some("1")
}

async fn boot() -> MetaStore {
    use testcontainers::runners::AsyncRunner;
    use testcontainers_modules::postgres::Postgres as PgImage;

    let pg = PgImage::default().start().await.expect("start postgres");
    let port = pg.get_host_port_ipv4(5432).await.expect("postgres port");
    let host = pg.get_host().await.expect("postgres host");
    let dsn = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let store = MetaStore::connect(&MetaStoreSettings {
        backend: "postgres".into(),
        dsn,
        min_connections: 1,
        max_connections: 5,
    })
    .await
    .expect("connect and migrate");
    std::mem::forget(pg);
    store
}

fn draft(
    id: &str,
    org: &Id,
    actor: &Id,
    expires_at: i64,
    contracts: &ResolvedDashboardContracts,
) -> DashboardDraft {
    let spec: DashboardAuthoringSpec = serde_json::from_str(VALID_AUTHORING).unwrap();
    let compiled = DashboardAuthoringCompiler::compile(&spec).unwrap();
    DashboardDraft {
        id: Id(id.into()),
        org_id: org.clone(),
        created_by: actor.clone(),
        authoring_version: 1,
        model_schema_version: compiled.dashboard_model_version,
        compiler_version: compiled.compiler_version,
        contract_binding_revision: contracts.binding.revision,
        authoring_schema_hash: contracts.binding.selection.authoring.schema_hash.clone(),
        model_schema_hash: contracts.binding.selection.model.schema_hash.clone(),
        visualization_schema_hash: contracts
            .binding
            .selection
            .visualization
            .schema_hash
            .clone(),
        authoring_spec: serde_json::to_value(spec).unwrap(),
        compiled_model: compiled.model,
        model_hash: compiled.model_hash,
        folder_id: None,
        status: DashboardDraftStatus::Ready,
        dashboard_id: None,
        warnings: Vec::new(),
        preflight: PreflightReport::default(),
        created_at: TimestampMicros(1_000),
        expires_at: TimestampMicros(expires_at),
        consumed_at: None,
    }
}

fn dashboard(id: &str, source: &DashboardDraft) -> Dashboard {
    Dashboard {
        id: Id(id.into()),
        org_id: source.org_id.clone(),
        folder_id: source.folder_id.clone(),
        uid: format!("uid-{id}"),
        title: source.compiled_model["title"].as_str().unwrap().to_string(),
        tags: vec!["ai".into()],
        model: source.compiled_model.clone(),
        version: 1,
        created_at: TimestampMicros(2_000),
        updated_at: TimestampMicros(2_000),
        created_by: source.created_by.clone(),
        updated_by: source.created_by.clone(),
    }
}

fn consume_request(source: &DashboardDraft, dashboard_id: &str) -> ConsumeDashboardDraft {
    ConsumeDashboardDraft {
        org_id: source.org_id.clone(),
        actor: source.created_by.clone(),
        draft_id: source.id.clone(),
        expected_hash: source.model_hash.clone(),
        compiler_version: source.compiler_version.clone(),
        now: TimestampMicros(2_000),
        dashboard: dashboard(dashboard_id, source),
    }
}

#[tokio::test]
async fn dashboard_draft_repository_scopes_expires_consumes_and_replays_atomically() {
    if skip_unless_enabled() {
        eprintln!("skipping Dashboard draft repository integration (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let repository = Arc::new(PgDashboardDraftRepository::new(store.pool.clone()));
    let registry = DashboardContractRegistryService::new(Arc::new(
        PgDashboardContractRepository::new(store.pool.clone()),
    ));
    registry.publish_builtins().await.unwrap();
    let contracts = registry.active().await.unwrap();
    let org = Id("org-a".into());
    let actor = Id("user-a".into());

    let expiring = draft("draft-expired", &org, &actor, 1_500, &contracts);
    repository.create(expiring.clone()).await.unwrap();
    assert!(
        repository
            .get(&Id("org-b".into()), &expiring.id, TimestampMicros(2_000))
            .await
            .is_err()
    );
    let expired = repository
        .get(&org, &expiring.id, TimestampMicros(2_000))
        .await
        .unwrap();
    assert_eq!(expired.status, DashboardDraftStatus::Expired);

    let ready = draft("draft-concurrent", &org, &actor, 10_000, &contracts);
    repository.create(ready.clone()).await.unwrap();
    let left = repository.consume_and_create(consume_request(&ready, "dashboard-left"));
    let right = repository.consume_and_create(consume_request(&ready, "dashboard-right"));
    let (left, right) = tokio::join!(left, right);
    let left = left.unwrap();
    let right = right.unwrap();
    assert_ne!(left.replayed(), right.replayed());
    assert_eq!(left.dashboard().id, right.dashboard().id);

    let replay = repository
        .consume_and_create(consume_request(&ready, "dashboard-retry"))
        .await
        .unwrap();
    assert!(matches!(replay, DraftConsumption::Replay(_)));
    assert_eq!(replay.dashboard().id, left.dashboard().id);

    let saved = repository
        .get(&org, &ready.id, TimestampMicros(3_000))
        .await
        .unwrap();
    assert_eq!(saved.status, DashboardDraftStatus::Consumed);
    assert_eq!(saved.dashboard_id, Some(left.dashboard().id.clone()));
    let row = sqlx::query("SELECT COUNT(*) AS count FROM dashboards WHERE org_id = $1")
        .bind(&org.0)
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(row.try_get::<i64, _>("count").unwrap(), 1);

    let raced = draft("draft-binding-race", &org, &actor, 10_000, &contracts);
    repository.create(raced.clone()).await.unwrap();
    sqlx::query(
        "UPDATE intelligence_capability_contract_bindings
         SET revision = revision + 1, updated_at_micros = updated_at_micros + 1
         WHERE capability_key = $1",
    )
    .bind(DASHBOARD_AUTHORING_CAPABILITY)
    .execute(&store.pool)
    .await
    .unwrap();
    let error = repository
        .consume_and_create(consume_request(&raced, "dashboard-binding-race"))
        .await
        .unwrap_err();
    match error {
        Error::Validation { issues, .. } => assert_eq!(issues[0].code, "DRAFT_STALE"),
        other => panic!("expected stale draft validation error, got {other}"),
    }
    let row = sqlx::query("SELECT COUNT(*) AS count FROM dashboards WHERE id = $1")
        .bind("dashboard-binding-race")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(row.try_get::<i64, _>("count").unwrap(), 0);
}

#[tokio::test]
async fn bundled_and_representative_stored_dashboards_pass_the_current_write_contract() {
    if skip_unless_enabled() {
        eprintln!("skipping Dashboard seed validation (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let rows = sqlx::query(
        "SELECT dashboards.uid, dashboards.model
         FROM dashboards
         JOIN organizations ON organizations.id = dashboards.org_id
         WHERE organizations.slug = '_sys' AND organizations.system
         ORDER BY dashboards.uid",
    )
    .fetch_all(&store.pool)
    .await
    .unwrap();
    assert!(rows.len() >= 5, "expected the bundled Dashboard catalog");
    for row in rows {
        let uid: String = row.try_get("uid").unwrap();
        let model: sqlx::types::Json<serde_json::Value> = row.try_get("model").unwrap();
        let issues = validate_dashboard_model(&model.0, DashboardValidationMode::Native);
        assert!(issues.is_empty(), "bundled Dashboard {uid}: {issues:?}");
    }

    let mut stored: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/contracts/dashboard/fixtures/valid/dashboard-v2-nested.json"
    )))
    .unwrap();
    stored["id"] = serde_json::json!("stored-dashboard-id");
    stored["version"] = serde_json::json!(7);
    stored["createdAt"] = serde_json::json!("2026-01-01T00:00:00Z");
    stored["updatedAt"] = serde_json::json!("2026-08-03T00:00:00Z");
    stored["createdBy"] = serde_json::json!("legacy-user");
    stored["updatedBy"] = serde_json::json!("current-user");
    assert!(
        validate_dashboard_model(&stored, DashboardValidationMode::Native).is_empty(),
        "representative stored v2 model must remain writable"
    );
}
