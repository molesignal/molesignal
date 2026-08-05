// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 跨集群事件总线存储层的 Postgres 往返（竖切验证）。
//!
//! 覆盖：
//! - outbox append（按 id 幂等）→ list_undelivered（org 过滤 + seq 升序）→ ack 游标
//!   （GREATEST 单调）→ prune；
//! - seen_events insert_if_absent 幂等（首见 true / 再见 false）+ sweep；
//! - Lamport 版本寄存器：bump_local 原子单调、get、adopt 采纳远端胜出版本、adopt 后本地
//!   再写胜出（配合 domain `wins` 验证收敛方向）；
//! - org 映射：未映射 get_for_remote → None（接收端拒收）、upsert / 双向查 / list / delete。
//!
//! 默认跳过：testcontainers 需 docker daemon，设 `MS_RUN_IT=1` 才真跑。
//!
//! ```bash
//! MS_RUN_IT=1 cargo test -p molesignal-infra --test it_cluster_events -- --nocapture
//! ```

use molesignal::{
    config::MetaStoreSettings,
    domain::federation::{CloudEvent, CudAction, ResourceKind, wins},
    infra::persistence::{
        MetaStore,
        repositories::cluster::events::{
            ClusterEventOutboxRepository, ClusterOrgLinkRepository,
            ClusterResourceVersionRepository, OrgLink, PgClusterEventOutboxRepository,
            PgClusterOrgLinkRepository, PgClusterResourceVersionRepository, PgSeenEventsRepository,
            SeenEventsRepository,
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
    std::mem::forget(pg); // keep container up for the test duration
    store
}

fn regex_event(id: &str, org: &str, res: &str, version: u64) -> CloudEvent {
    CloudEvent::new(
        id.to_string(),
        "clusterA",
        ResourceKind::RegexPattern,
        CudAction::Created,
        org,
        res,
        version,
        serde_json::json!({"id": res, "name": res}),
        "2026-06-15T00:00:00Z".to_string(),
    )
}

#[tokio::test]
async fn outbox_cursor_prune_and_seen_dedup() {
    if skip_unless_enabled() {
        eprintln!("skipping it_cluster_events (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let outbox = PgClusterEventOutboxRepository::new(store.pool.clone());
    let org_a = Id::from_string("orgA");
    let cluster_b = Id::from_string("clusterB");

    // append 3 events for orgA.
    for i in 1..=3 {
        outbox
            .append(
                &org_a,
                &regex_event(&format!("evt-{i}"), "orgA", &format!("rp-{i}"), i),
            )
            .await
            .expect("append");
    }
    // 未确认游标初始为 0。
    assert_eq!(outbox.acked_seq(&cluster_b).await.unwrap(), 0);

    // list_undelivered(after=0) → 3 条，seq 升序。
    let rows = outbox
        .list_undelivered(0, &["orgA".into()], 100)
        .await
        .unwrap();
    assert_eq!(rows.len(), 3);
    let seqs: Vec<i64> = rows.iter().map(|r| r.seq).collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "seq ascending");
    let max_seq = *seqs.iter().max().unwrap();

    // append 幂等：同 id 再插不增行。
    outbox
        .append(&org_a, &regex_event("evt-1", "orgA", "rp-1", 1))
        .await
        .expect("re-append");
    assert_eq!(
        outbox
            .list_undelivered(0, &["orgA".into()], 100)
            .await
            .unwrap()
            .len(),
        3,
        "duplicate id must not add a row"
    );

    // org 过滤：orgB 没有事件。
    assert!(
        outbox
            .list_undelivered(0, &["orgB".into()], 100)
            .await
            .unwrap()
            .is_empty()
    );

    // ack 推进游标到 max_seq；GREATEST → 回退的 ack 不降游标。
    outbox.ack(&cluster_b, max_seq).await.unwrap();
    assert_eq!(outbox.acked_seq(&cluster_b).await.unwrap(), max_seq);
    outbox.ack(&cluster_b, 1).await.unwrap();
    assert_eq!(
        outbox.acked_seq(&cluster_b).await.unwrap(),
        max_seq,
        "ack is monotonic"
    );

    // 游标之后无未投递。
    assert!(
        outbox
            .list_undelivered(max_seq, &["orgA".into()], 100)
            .await
            .unwrap()
            .is_empty()
    );

    // prune 删到 max_seq → 清空。
    let pruned = outbox.prune(max_seq).await.unwrap();
    assert_eq!(pruned, 3);
    assert_eq!(outbox.max_seq().await.unwrap(), 0);

    // seen_events 幂等去重。
    let seen = PgSeenEventsRepository::new(store.pool.clone());
    assert!(
        seen.insert_if_absent("e1").await.unwrap(),
        "first sighting → true"
    );
    assert!(
        !seen.insert_if_absent("e1").await.unwrap(),
        "duplicate → false"
    );
    // remove 撤销去重标记 → 可再次首见（瞬时失败重投路径）。
    seen.remove("e1").await.unwrap();
    assert!(
        seen.insert_if_absent("e1").await.unwrap(),
        "after remove → first again"
    );
    assert!(seen.insert_if_absent("e2").await.unwrap());
    // sweep（cutoff 在未来）→ 清掉两条，e1 可再插。
    let future = TimestampMicros(TimestampMicros::now().0 + 1_000_000_000);
    assert_eq!(seen.sweep(future).await.unwrap(), 2);
    assert!(seen.insert_if_absent("e1").await.unwrap());
}

#[tokio::test]
async fn lamport_version_converges() {
    if skip_unless_enabled() {
        eprintln!("skipping it_cluster_events (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let ver = PgClusterResourceVersionRepository::new(store.pool.clone());
    let org = Id::from_string("orgA");
    let kind = ResourceKind::RegexPattern.as_str();

    // bump_local 原子单调：1,2,3。
    assert_eq!(
        ver.bump_local(kind, &org, "rp-1", "clusterA")
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        ver.bump_local(kind, &org, "rp-1", "clusterA")
            .await
            .unwrap(),
        2
    );
    assert_eq!(
        ver.bump_local(kind, &org, "rp-1", "clusterA")
            .await
            .unwrap(),
        3
    );
    let (v, w) = ver.get(kind, &org, "rp-1").await.unwrap().unwrap();
    assert_eq!((v, w.as_str()), (3, "clusterA"));
    // 不同资源各自独立。
    assert_eq!(
        ver.bump_local(kind, &org, "rp-2", "clusterA")
            .await
            .unwrap(),
        1
    );
    // 未知资源 → None。
    assert!(ver.get(kind, &org, "nope").await.unwrap().is_none());

    // 远端 clusterB v5 > 本地 v3 → 胜出 → adopt。
    let local = ver.get(kind, &org, "rp-1").await.unwrap().unwrap();
    assert!(wins((5, "clusterB"), (local.0 as u64, local.1.as_str())));
    ver.adopt(kind, &org, "rp-1", 5, "clusterB").await.unwrap();
    let (v, w) = ver.get(kind, &org, "rp-1").await.unwrap().unwrap();
    assert_eq!((v, w.as_str()), (5, "clusterB"));

    // 旧版本不覆盖；同版本 writer 字典序 tiebreak。
    assert!(!wins((4, "clusterC"), (5, "clusterB")));
    assert!(wins((5, "clusterC"), (5, "clusterB")));
    assert!(!wins((5, "clusterA"), (5, "clusterB")));

    // adopt 后本地再写：从采纳的 5 → 6（writer 回 clusterA），下一轮可胜过 clusterB v5。
    assert_eq!(
        ver.bump_local(kind, &org, "rp-1", "clusterA")
            .await
            .unwrap(),
        6
    );
    assert!(wins((6, "clusterA"), (5, "clusterB")));
}

#[tokio::test]
async fn org_link_mapping_and_reject() {
    if skip_unless_enabled() {
        eprintln!("skipping it_cluster_events (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let links = PgClusterOrgLinkRepository::new(store.pool.clone());
    let cluster_a = Id::from_string("clusterA");

    // 未映射 → None（接收端据此拒收）。
    assert!(
        links
            .get_for_remote(&cluster_a, &Id::from_string("orgRemote"))
            .await
            .unwrap()
            .is_none()
    );

    // upsert：本地 orgLocal ← 远端 orgRemote（带 per-org token）。
    links
        .upsert(OrgLink {
            remote_cluster_id: cluster_a.clone(),
            local_org_id: Id::from_string("orgLocal"),
            remote_org_id: Id::from_string("orgRemote"),
            token_secret_ref: Some("env:TOK".into()),
        })
        .await
        .unwrap();

    // 接收端：远端 org → 本地 org。
    let l = links
        .get_for_remote(&cluster_a, &Id::from_string("orgRemote"))
        .await
        .unwrap()
        .expect("mapped");
    assert_eq!(l.local_org_id.0, "orgLocal");
    assert_eq!(l.token_secret_ref.as_deref(), Some("env:TOK"));
    // 发送端：本地 org → 远端 org。
    let l2 = links
        .get_for_local(&cluster_a, &Id::from_string("orgLocal"))
        .await
        .unwrap()
        .expect("mapped");
    assert_eq!(l2.remote_org_id.0, "orgRemote");

    // upsert 覆盖（改 remote_org + 清 token）。
    links
        .upsert(OrgLink {
            remote_cluster_id: cluster_a.clone(),
            local_org_id: Id::from_string("orgLocal"),
            remote_org_id: Id::from_string("orgRemote2"),
            token_secret_ref: None,
        })
        .await
        .unwrap();
    let l3 = links
        .get_for_local(&cluster_a, &Id::from_string("orgLocal"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(l3.remote_org_id.0, "orgRemote2");
    assert!(l3.token_secret_ref.is_none());

    // list 单条；delete 后双向查均 None。
    assert_eq!(links.list(&cluster_a).await.unwrap().len(), 1);
    links
        .delete(&cluster_a, &Id::from_string("orgLocal"))
        .await
        .unwrap();
    assert!(
        links
            .get_for_local(&cluster_a, &Id::from_string("orgLocal"))
            .await
            .unwrap()
            .is_none()
    );
}
