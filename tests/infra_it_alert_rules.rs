// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `alert_rules` Postgres 往返：重点验证 `kind` / `anomaly_params_json` 两列在
//! create → get → list_enabled → update 全链路上正确落库与还原（F3 anomaly 规则页）。
//!
//! 默认跳过：testcontainers 需 docker daemon，设 `MS_RUN_IT=1` 才真跑。
//!
//! ```bash
//! MS_RUN_IT=1 cargo test -p molesignal-infra --test it_alert_rules -- --nocapture
//! ```

use std::collections::BTreeMap;

use molesignal::{
    config::MetaStoreSettings,
    domain::{
        alerting::{
            anomaly::AnomalyParams,
            repositories::AlertRuleRepository,
            rule::{AlertQuery, AlertRule, AlertRuleKind, AlertTrigger, ComparisonOp, RuleState},
        },
        query::{QueryLanguage, StreamHint},
        stream::StreamType,
    },
    infra::persistence::{MetaStore, repositories::alert_rules::PgAlertRuleRepository},
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

fn base_rule(id: &str, org: &str, kind: AlertRuleKind, params: Option<AnomalyParams>) -> AlertRule {
    let now = TimestampMicros::now();
    AlertRule {
        id: Id::from_string(id),
        org_id: Id::from_string(org),
        name: format!("rule-{id}"),
        description: String::new(),
        enabled: true,
        kind,
        query: AlertQuery {
            language: QueryLanguage::Sql,
            statement: "SELECT avg(latency) FROM logs".into(),
            period_secs: 60,
            stream: Some(StreamHint {
                name: "logs".into(),
                stream_type: StreamType::Logs,
            }),
        },
        anomaly_params: params,
        thresholds: vec![],
        severity: None,
        trigger: AlertTrigger {
            operator: ComparisonOp::Gt,
            threshold: 0.0,
            for_periods: 1,
            silence_secs: 0,
        },
        escalation_policy_id: Id::from_string("default"),
        labels: BTreeMap::new(),
        annotations: BTreeMap::new(),
        last_eval_at: None,
        last_state: RuleState::Healthy,
        created_at: now,
        updated_at: now,
    }
}

#[tokio::test]
async fn alert_rule_kind_and_anomaly_params_roundtrip() {
    if skip_unless_enabled() {
        eprintln!("skipping it_alert_rules (set MS_RUN_IT=1)");
        return;
    }
    let store = boot().await;
    let repo = PgAlertRuleRepository::new(store.pool.clone());
    let org = "org-anom";

    // 1. anomaly 规则：kind + anomaly_params 往返。
    let params = AnomalyParams {
        algorithm: "mad".into(),
        lookback_days: 14,
        k: 2.5,
        alpha: 0.3,
        weekly_seasonality: false,
    };
    repo.create(base_rule(
        "anom-1",
        org,
        AlertRuleKind::Anomaly,
        Some(params.clone()),
    ))
    .await
    .expect("create anomaly");

    let got = repo.get(&Id::from_string("anom-1")).await.expect("get");
    assert_eq!(got.kind, AlertRuleKind::Anomaly);
    assert_eq!(got.anomaly_params.as_ref(), Some(&params));

    // 2. scheduled 规则（默认 kind）：anomaly_params 落 NULL → None。
    repo.create(base_rule("sched-1", org, AlertRuleKind::Scheduled, None))
        .await
        .expect("create scheduled");
    let sched = repo.get(&Id::from_string("sched-1")).await.expect("get");
    assert_eq!(sched.kind, AlertRuleKind::Scheduled);
    assert!(sched.anomaly_params.is_none());

    // 3. list_enabled 两条都在，且各自 kind/params 正确。
    let enabled = repo.list_enabled().await.expect("list_enabled");
    let a = enabled
        .iter()
        .find(|r| r.id.0 == "anom-1")
        .expect("anom in list");
    assert_eq!(a.kind, AlertRuleKind::Anomaly);
    assert_eq!(a.anomaly_params.as_ref().map(|p| p.lookback_days), Some(14));

    // 4. update：改 k → 还原。
    let mut updated = got;
    updated.anomaly_params = Some(AnomalyParams {
        algorithm: "mad".into(),
        lookback_days: 14,
        k: 4.0,
        alpha: 0.3,
        weekly_seasonality: false,
    });
    repo.update(updated).await.expect("update");
    let after = repo.get(&Id::from_string("anom-1")).await.expect("get2");
    assert_eq!(after.anomaly_params.map(|p| p.k), Some(4.0));
}
