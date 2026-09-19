// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use molesignal::{
    app::dashboard::contract_registry::DashboardContractRegistryService,
    config::MetaStoreSettings,
    domain::dashboard::contract_registry::{
        DASHBOARD_AUTHORING_CAPABILITY, DashboardContractRepository,
    },
    infra::persistence::{
        MetaStore, repositories::dashboard_contract_registry::PgDashboardContractRepository,
    },
    shared::{
        contracts::{canonical_json_bytes, sha256_hex},
        time::TimestampMicros,
    },
};
use serde_json::json;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dashboard_contract_registry_publishes_activates_rolls_back_and_serializes_revisions() {
    if skip_unless_enabled() {
        eprintln!("skipping Dashboard contract registry integration (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let repository = Arc::new(PgDashboardContractRepository::new(store.pool.clone()));
    let registry = DashboardContractRegistryService::new(repository.clone());

    let first = registry.publish_builtins().await.unwrap();
    let repeated = registry.publish_builtins().await.unwrap();
    assert_eq!(first.binding.revision, 1);
    assert_eq!(repeated.binding.revision, 1);
    let original = repository
        .load_active(DASHBOARD_AUTHORING_CAPABILITY)
        .await
        .unwrap();
    assert_eq!(
        original.documents.model.reference(),
        original.binding.selection.model
    );

    let mut conflict = original.documents.model.clone();
    conflict.document["description"] = json!("conflicting immutable content");
    conflict.schema_hash = sha256_hex(canonical_json_bytes(&conflict.document));
    assert!(
        repository
            .publish_builtin(
                &[conflict],
                &original.binding.selection,
                TimestampMicros::now(),
            )
            .await
            .is_err()
    );

    let mut alternate_model = original.documents.model.clone();
    alternate_model.version += 100;
    let alternate_reference = alternate_model.reference();
    repository
        .publish_builtin(
            &[alternate_model],
            &original.binding.selection,
            TimestampMicros::now(),
        )
        .await
        .unwrap();
    let mut alternate = original.binding.selection.clone();
    alternate.model = alternate_reference;
    let activated = registry.activate(alternate.clone()).await.unwrap();
    assert_eq!(activated.binding.revision, 2);
    assert_eq!(activated.binding.selection.model, alternate.model);

    let rolled_back = registry
        .activate(original.binding.selection.clone())
        .await
        .unwrap();
    assert_eq!(rolled_back.binding.revision, 3);
    assert_eq!(
        rolled_back.binding.selection.model,
        original.binding.selection.model
    );

    let mut invalid = original.binding.selection.clone();
    invalid.model.schema_hash = "0".repeat(64);
    assert!(
        repository
            .activate(&invalid, TimestampMicros::now())
            .await
            .is_err()
    );
    assert_eq!(
        repository
            .load_active(DASHBOARD_AUTHORING_CAPABILITY)
            .await
            .unwrap()
            .binding
            .revision,
        3
    );

    let left = repository.activate(&alternate, TimestampMicros::now());
    let right = repository.activate(&original.binding.selection, TimestampMicros::now());
    let (left, right) = tokio::join!(left, right);
    let mut revisions = [left.unwrap().revision, right.unwrap().revision];
    revisions.sort_unstable();
    assert_eq!(revisions, [4, 5]);
    let final_bundle = repository
        .load_active(DASHBOARD_AUTHORING_CAPABILITY)
        .await
        .unwrap();
    assert_eq!(final_bundle.binding.revision, 5);
    assert_eq!(
        final_bundle.documents.model.reference(),
        final_bundle.binding.selection.model
    );
}
