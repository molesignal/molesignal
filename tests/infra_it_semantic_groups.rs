// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `semantic_groups` + `incident_groups` Postgres 往返：
//! - PgSemanticGroupRepository CRUD（matchers / group_by JSONB 往返）+ org 隔离 + list_enabled。
//! - PgIncidentGroupRepository.upsert_for_incident 分组语义：同 (scope, fp) 窗口内并组
//!   count++、不同 fp 另起一组（此前无 caller 也无测试，semantic groups 接它后补上）。
//!
//! 默认跳过：testcontainers 需 docker daemon，设 `MS_RUN_IT=1` 才真跑。
//!
//! ```bash
//! MS_RUN_IT=1 cargo test -p molesignal-infra --test it_semantic_groups -- --nocapture
//! ```

use molesignal::{
    config::MetaStoreSettings,
    domain::alerting::{
        incident_group::IncidentGroupRepository,
        semantic_group::{LabelMatcher, MatchOp, SemanticGroup, SemanticGroupRepository},
    },
    infra::persistence::{
        MetaStore,
        repositories::{
            incidents::groups::PgIncidentGroupRepository,
            semantic_groups::PgSemanticGroupRepository,
        },
    },
    shared::{ids::Id, time::TimestampMicros},
};
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::postgres::Postgres as PgImage;

fn skip_unless_enabled() -> bool {
    std::env::var("MS_RUN_IT").ok().as_deref() != Some("1")
}

async fn boot() -> MetaStore {
    let pg = PgImage::default().start().await.expect("start pg");
    let port = pg.get_host_port_ipv4(5432).await.expect("pg port");
    let host = pg.get_host().await.expect("pg host");
    let dsn = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    let store = MetaStore::connect(&MetaStoreSettings {
        backend: "postgres".into(),
        dsn,
        min_connections: 1,
        max_connections: 5,
    })
    .await
    .expect("connect + migrate");
    std::mem::forget(pg);
    store
}

fn group(id: &str, org: &str, name: &str, enabled: bool) -> SemanticGroup {
    group_at(id, org, name, enabled, TimestampMicros::now())
}

fn group_at(id: &str, org: &str, name: &str, enabled: bool, at: TimestampMicros) -> SemanticGroup {
    SemanticGroup {
        id: Id::from_string(id),
        org_id: Id::from_string(org),
        name: name.into(),
        enabled,
        matchers: vec![LabelMatcher {
            label: "severity".into(),
            op: MatchOp::Eq,
            value: "critical".into(),
        }],
        group_by: vec!["service".into(), "region".into()],
        created_at: at,
        updated_at: at,
    }
}

#[tokio::test]
async fn semantic_group_crud_roundtrip_and_org_isolation() {
    if skip_unless_enabled() {
        eprintln!("skipping it_semantic_groups (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let repo = PgSemanticGroupRepository::new(store.pool.clone());

    repo.create(group("g1", "orgA", "crit-by-service", true))
        .await
        .expect("create");

    // 1. get 往返：matchers / group_by JSONB 完整还原。
    let got = repo.get(&Id::from_string("g1")).await.expect("get");
    assert_eq!(got.name, "crit-by-service");
    assert_eq!(got.matchers.len(), 1);
    assert_eq!(got.matchers[0].op, MatchOp::Eq);
    assert_eq!(
        got.group_by,
        vec!["service".to_string(), "region".to_string()]
    );

    // 2. update：改名 + 禁用 + group_by。
    let mut updated = got;
    updated.name = "renamed".into();
    updated.enabled = false;
    updated.group_by = vec!["service".into()];
    repo.update(updated).await.expect("update");
    let after = repo.get(&Id::from_string("g1")).await.expect("get2");
    assert_eq!(after.name, "renamed");
    assert!(!after.enabled);
    assert_eq!(after.group_by, vec!["service".to_string()]);

    // 3. list_enabled 不含已禁用的 g1；另起一个 enabled 的 g2 应出现。
    repo.create(group("g2", "orgA", "g2", true))
        .await
        .expect("create g2");
    let enabled = repo
        .list_enabled(&Id::from_string("orgA"))
        .await
        .expect("list_enabled");
    assert!(enabled.iter().any(|g| g.id.0 == "g2"));
    assert!(!enabled.iter().any(|g| g.id.0 == "g1"));

    // 4. org 隔离：另一个 org 看不到 orgA 的 group。
    let other = repo
        .list(&Id::from_string("orgB"))
        .await
        .expect("list orgB");
    assert!(other.is_empty());

    // 5. delete。
    repo.delete(&Id::from_string("g2")).await.expect("delete");
    assert!(repo.get(&Id::from_string("g2")).await.is_err());
}

#[tokio::test]
async fn incident_group_upsert_groups_by_scope_and_fingerprint() {
    if skip_unless_enabled() {
        eprintln!("skipping it_semantic_groups (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let repo = PgIncidentGroupRepository::new(store.pool.clone());
    let org = Id::from_string("orgG");
    let scope = Id::from_string("sg-1");
    let t0 = TimestampMicros::now();
    let window = 3600;

    // 同 (scope, fp) 在窗口内：并入同一组，count 累加。
    let g1 = repo
        .upsert_for_incident(&org, &scope, "service=api;region=us", t0, window)
        .await
        .expect("upsert1");
    assert_eq!(g1.incident_count, 1);
    let t1 = TimestampMicros(t0.0 + 60_000_000);
    let g2 = repo
        .upsert_for_incident(&org, &scope, "service=api;region=us", t1, window)
        .await
        .expect("upsert2");
    assert_eq!(g1.id, g2.id, "same scope+fp within window => same group");
    assert_eq!(g2.incident_count, 2);

    // 不同 fp（不同 group_by 取值）：另起一组。
    let g3 = repo
        .upsert_for_incident(&org, &scope, "service=web;region=eu", t1, window)
        .await
        .expect("upsert3");
    assert_ne!(g3.id, g1.id, "different fingerprint => different group");
    assert_eq!(g3.incident_count, 1);
}

#[tokio::test]
async fn list_enabled_orders_by_definition_not_name() {
    if skip_unless_enabled() {
        eprintln!("skipping it_semantic_groups (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let repo = PgSemanticGroupRepository::new(store.pool.clone());
    let org = "orgP";
    // "zzz" 创建在前（更早 created_at），"aaa" 在后。precedence 必须是创建顺序，
    // 不能被字母序 (aaa < zzz) 颠倒——evaluator 取第一条命中决定分组归属。
    repo.create(group_at("p-zzz", org, "zzz", true, TimestampMicros(1_000)))
        .await
        .expect("create zzz");
    repo.create(group_at("p-aaa", org, "aaa", true, TimestampMicros(2_000)))
        .await
        .expect("create aaa");
    let enabled = repo
        .list_enabled(&Id::from_string(org))
        .await
        .expect("list_enabled");
    assert_eq!(
        enabled.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
        vec!["zzz", "aaa"],
        "precedence must follow definition (created_at) order, not name"
    );
}

#[tokio::test]
async fn create_many_is_atomic_rolls_back_on_failure() {
    if skip_unless_enabled() {
        eprintln!("skipping it_semantic_groups (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let repo = PgSemanticGroupRepository::new(store.pool.clone());
    let org = "orgM";

    // happy path：两条都落库。
    let n = repo
        .create_many(vec![
            group("m-1", org, "g1", true),
            group("m-2", org, "g2", true),
        ])
        .await
        .expect("create_many ok");
    assert_eq!(n, 2);
    assert_eq!(repo.list(&Id::from_string(org)).await.unwrap().len(), 2);

    // 故障注入：第二条与第一条 id 冲突 → PK 违例 → 整批回滚，一条都不应落。
    let org2 = "orgM2";
    let res = repo
        .create_many(vec![
            group("dup", org2, "a", true),
            group("dup", org2, "b", true),
        ])
        .await;
    assert!(res.is_err(), "duplicate id must fail the batch");
    assert_eq!(
        repo.list(&Id::from_string(org2)).await.unwrap().len(),
        0,
        "failed import must roll back entirely (no partial rows)"
    );
}
