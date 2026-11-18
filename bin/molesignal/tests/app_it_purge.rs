// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Deleting an organization or a stream only marks it; the purge removes the rest. The foreign
//! keys follow the rules the purge relies on. Runs against PostgreSQL with `MS_RUN_IT=1`.

mod common;

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use domain::{iam::OrganizationRepository, stream::StreamRepository};
use kernel::ids::Id;
use postgres::{
    MetaStore,
    repositories::{
        file_catalog::{DatasetPurgeRepository, PgDatasetPurge},
        organizations::{
            OrganizationPurgeRepository, PgOrganizationPurge, PgOrganizationRepository,
        },
        streams::PgStreamRepository,
    },
};
use settings::MetaDatabaseSettings;
use sqlx::{PgPool, Row};

/// One organization's rows along the chains the purge order and its triggers depend on: a
/// synthetic monitor with its revision, task, result and states at an organization location; a
/// status page component an incident names; a policy a role attaches; an alert rule with state;
/// a stream down to a catalog artifact.
const ROWS: &str = "
INSERT INTO organizations (id, name, slug, created_at) VALUES ('{o}', '{o}', '{o}', now());
INSERT INTO synthetic_probe_locations (id, organization_id, name, code, scope, execution, created_at, updated_at)
    VALUES ('loc-{o}', '{o}', 'Location', 'loc-{o}', 'organization', 'agent_pool', now(), now());
INSERT INTO synthetic_monitors (id, organization_id, name, kind, created_by, created_at, updated_at)
    VALUES ('mon-{o}', '{o}', 'Monitor', 'http', 'user-{o}', now(), now());
INSERT INTO synthetic_monitor_revisions (id, organization_id, monitor_id, revision_number, spec, schedule,
        timeout_millis, freshness_seconds, location_policy, location_ids, created_by, created_at, content_hash)
    VALUES ('rev-{o}', '{o}', 'mon-{o}', 1, '{}', '{}', 1000, 60, '{}', ARRAY['loc-{o}'], 'user-{o}', now(), 'hash');
UPDATE synthetic_monitors SET active_revision_id = 'rev-{o}' WHERE id = 'mon-{o}';
INSERT INTO synthetic_probe_tasks (id, organization_id, monitor_id, monitor_revision_id, location_id, spec,
        scheduled_at, deadline_at, timeout_millis, max_attempts, created_at, updated_at)
    VALUES ('task-{o}', '{o}', 'mon-{o}', 'rev-{o}', 'loc-{o}', '{}', now(), now() + interval '1 minute', 1000, 1, now(), now());
INSERT INTO synthetic_results (id, organization_id, monitor_id, monitor_revision_id, location_id, task_id,
        scheduled_at, started_at, finished_at, received_at, outcome, protocol_version)
    VALUES ('res-{o}', '{o}', 'mon-{o}', 'rev-{o}', 'loc-{o}', 'task-{o}', now(), now(), now(), now(), 'healthy', 1);
INSERT INTO synthetic_state_transitions (id, organization_id, monitor_id, monitor_revision_id, location_id,
        result_id, previous_state, current_state, correlation_key, started_at, created_at)
    VALUES ('tr-{o}', '{o}', 'mon-{o}', 'rev-{o}', 'loc-{o}', 'res-{o}', 'unknown', 'healthy', 'key', now(), now());
INSERT INTO synthetic_monitor_location_states (organization_id, monitor_id, monitor_revision_id, location_id,
        last_result_id, updated_at)
    VALUES ('{o}', 'mon-{o}', 'rev-{o}', 'loc-{o}', 'res-{o}', now());
INSERT INTO status_pages (id, org_id, name, slug, created_at, updated_at)
    VALUES ('page-{o}', '{o}', 'Page', 'page-{o}', now(), now());
INSERT INTO status_page_components (id, org_id, status_page_id, name, created_at, updated_at)
    VALUES ('comp-{o}', '{o}', 'page-{o}', 'API', now(), now());
INSERT INTO status_page_incidents (id, org_id, status_page_id, kind, title, impact, status, started_at,
        created_at, updated_at, publication_state, component_ids)
    VALUES ('inc-{o}', '{o}', 'page-{o}', 'incident', 'Down', 'minor', 'investigating', now(), now(), now(),
        'draft', ARRAY['comp-{o}']);
INSERT INTO iam_policies (id, organization_id, policy_key, name, document, permissions, created_at, updated_at)
    VALUES ('pol-{o}', '{o}', 'readers', 'Readers', '{}', ARRAY['dashboards.read'], now(), now());
INSERT INTO iam_roles (id, org_id, role_key, name, policy_ids, created_at, updated_at)
    VALUES ('role-{o}', '{o}', 'custom', 'Custom', ARRAY['pol-{o}'], now(), now());
INSERT INTO alert_rules (id, org_id, name, description, enabled, query, trigger, escalation_policy_id, labels,
        annotations, last_state, created_at, updated_at)
    VALUES ('rule-{o}', '{o}', 'Rule', '', TRUE, '{}', '{}', 'policy-{o}', '{}', '{}', '{}', now(), now());
INSERT INTO alert_rule_eval_state (rule_id, last_eval_at) VALUES ('rule-{o}', now());
INSERT INTO logical_streams (id, org_id, name, stream_type, schema, created_at, updated_at)
    VALUES ('stream-{o}', '{o}', 'app', 'logs', '{}', now(), now());
INSERT INTO physical_datasets (org_id, id, logical_stream_id, dataset_type, created_at, updated_at)
    VALUES ('{o}', 'ds-{o}', 'stream-{o}', 'logs', now(), now());
INSERT INTO catalog_segments (org_id, id, dataset_id, partition_start, partition_end, min_event, max_event,
        row_count, state, visible_from_version, created_at)
    VALUES ('{o}', 'seg-{o}', 'ds-{o}', now(), now(), now(), now(), 1, 'active', 1, now());
INSERT INTO catalog_artifacts (org_id, id, segment_id, role, artifact_type, object_key, size_bytes, checksum,
        state, created_at, updated_at)
    VALUES ('{o}', 'art-{o}', 'seg-{o}', 'primary', 'parquet', 'artifacts/v1/{o}/logs/ds/x', 1, 'c', 'active',
        now(), now())";

async fn database() -> (PgPool, Option<impl Sized>) {
    let (dsn, container) = common::test_database().await;

    let store = MetaStore::connect(&MetaDatabaseSettings {
        dsn,
        ..MetaDatabaseSettings::default()
    })
    .await
    .expect("migrate a test database");

    (store.pool, container)
}

async fn fill(pool: &PgPool, organization: &str) {
    for statement in ROWS.split(";\n").map(str::trim).filter(|s| !s.is_empty()) {
        sqlx::query(&statement.replace("{o}", organization))
            .execute(pool)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }
}

/// Rows per table that has an organization column, for one organization.
async fn rows_of(pool: &PgPool, organization: &str) -> BTreeMap<String, i64> {
    let tables = sqlx::query(
        "SELECT DISTINCT ON (class.relname) class.relname::TEXT AS name, attribute.attname::TEXT AS col
           FROM pg_class class
           JOIN pg_namespace namespace ON namespace.oid = class.relnamespace
           JOIN pg_attribute attribute ON attribute.attrelid = class.oid
          WHERE namespace.nspname = current_schema() AND class.relkind = 'r'
            AND attribute.attname IN ('org_id', 'organization_id') AND NOT attribute.attisdropped
          ORDER BY class.relname, attribute.attname",
    )
    .fetch_all(pool)
    .await
    .expect("tables with an organization column");

    let mut rows = BTreeMap::new();

    for table in tables {
        let name: String = table.get("name");
        let column: String = table.get("col");

        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM \"{name}\" WHERE \"{column}\" = $1"
        ))
        .bind(organization)
        .fetch_one(pool)
        .await
        .expect("count rows");

        if count > 0 {
            rows.insert(name, count);
        }
    }

    rows
}

#[tokio::test]
async fn every_foreign_key_follows_the_rules_the_purge_relies_on() {
    if common::skip_unless_enabled() {
        return;
    }

    let (pool, _container) = database().await;

    let keys = sqlx::query(
        "SELECT key.conname::TEXT AS name,
                child.relname::TEXT AS child,
                parent.relname::TEXT AS parent,
                key.confdeltype::TEXT AS action,
                EXISTS (
                    SELECT 1 FROM pg_index candidate
                     WHERE candidate.indrelid = key.conrelid
                       AND candidate.indpred IS NULL
                       AND (SELECT array_agg(column_number ORDER BY column_number)
                              FROM unnest(candidate.indkey[0:array_length(key.conkey, 1) - 1])
                                   AS index_column(column_number))
                         = (SELECT array_agg(column_number ORDER BY column_number)
                              FROM unnest(key.conkey) AS key_column(column_number))
                ) AS indexed
           FROM pg_constraint key
           JOIN pg_class child ON child.oid = key.conrelid
           JOIN pg_class parent ON parent.oid = key.confrelid
          WHERE key.contype = 'f'",
    )
    .fetch_all(&pool)
    .await
    .expect("foreign keys");

    assert!(keys.len() > 50, "{} foreign keys read", keys.len());

    let mut broken = Vec::new();

    for key in &keys {
        let name: String = key.get("name");
        let parent: String = key.get("parent");
        let action: String = key.get("action");

        if !key.get::<bool, _>("indexed") {
            broken.push(format!("{name}: no index leads with its columns"));
        }

        if parent == "organizations" {
            broken.push(format!(
                "{name}: refers to organizations, which the purge removes last"
            ));
        }

        if parent == "users" && (action == "a" || action == "r") {
            broken.push(format!("{name}: keeps a user from being deleted"));
        }
    }

    assert!(broken.is_empty(), "{broken:#?}");
}

#[tokio::test]
async fn a_deleted_organization_is_purged_and_no_other() {
    if common::skip_unless_enabled() {
        return;
    }

    let (pool, _container) = database().await;
    fill(&pool, "orga").await;
    fill(&pool, "orgb").await;

    let kept = rows_of(&pool, "orgb").await;
    let organizations = PgOrganizationRepository::new(pool.clone());
    organizations
        .delete(&Id::from_string("orga"))
        .await
        .expect("delete");

    assert!(organizations.get(&Id::from_string("orga")).await.is_err());
    assert!(organizations.get_by_slug("orga").await.is_err());

    // The slug is free at once, before the purge.
    sqlx::query(
        "INSERT INTO organizations (id, name, slug, created_at) VALUES ('orgc', 'c', 'orga', now())",
    )
    .execute(&pool)
    .await
    .expect("reuse the slug");

    let purge = PgOrganizationPurge::new(pool.clone());
    let deleted = purge.deleted().await.expect("deleted organizations");

    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0].id.as_str(), "orga");

    let mut passes = 0;

    loop {
        let deadline = Instant::now() + Duration::from_secs(60);
        let progress = purge
            .purge_rows(&Id::from_string("orga"), 2, deadline)
            .await
            .expect("purge pass");

        passes += 1;

        if progress.finished {
            break;
        }

        assert!(passes < 5, "no pass finished");
    }

    purge
        .remove(&Id::from_string("orga"))
        .await
        .expect("remove");

    assert_eq!(rows_of(&pool, "orga").await, BTreeMap::new());
    assert_eq!(rows_of(&pool, "orgb").await, kept);
    assert!(
        purge
            .deleted()
            .await
            .expect("deleted organizations")
            .is_empty()
    );

    let state: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM alert_rule_eval_state WHERE rule_id = 'rule-orga'",
    )
    .fetch_one(&pool)
    .await
    .expect("rule state");

    assert_eq!(state, 0);
}

#[tokio::test]
async fn a_deleted_stream_leaves_its_catalog_rows_to_the_purge() {
    if common::skip_unless_enabled() {
        return;
    }

    let (pool, _container) = database().await;
    fill(&pool, "orga").await;

    PgStreamRepository::new(pool.clone())
        .delete(&Id::from_string("stream-orga"))
        .await
        .expect("delete the stream");

    let state: String =
        sqlx::query_scalar("SELECT state FROM physical_datasets WHERE id = 'ds-orga'")
            .fetch_one(&pool)
            .await
            .expect("the dataset waits for the purge");

    assert_eq!(state, "deleting");

    let progress = PgDatasetPurge::new(pool.clone())
        .purge_deleting(1, Instant::now() + Duration::from_secs(60))
        .await
        .expect("purge");

    assert!(progress.rows_deleted >= 3, "{progress:?}");

    for table in ["physical_datasets", "catalog_segments", "catalog_artifacts"] {
        let count: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM {table} WHERE org_id = 'orga'"
        ))
        .fetch_one(&pool)
        .await
        .expect("count");

        assert_eq!(count, 0, "{table}");
    }
}
