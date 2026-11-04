// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use serde_json::json;

use super::*;
use crate::{
    domain::pipeline::limit::{
        DAY_MICROS, MAX_QUOTA_BUCKETS, MAX_THROTTLE_GROUPS, MemoryLimitUsageLedger, OTHER_BUCKET,
    },
    shared::Error,
};

const SECOND: i64 = 1_000_000;

fn quota(value: Value) -> QuotaSpec {
    let mut spec: QuotaSpec = serde_json::from_value(value).unwrap();
    spec.prepare().unwrap();
    spec
}

fn throttle(value: Value) -> ThrottleSpec {
    let spec: ThrottleSpec = serde_json::from_value(value).unwrap();
    spec.validate().unwrap();
    spec
}

fn org() -> Id {
    Id("org".into())
}

fn pipeline() -> Id {
    Id("p".into())
}

async fn admits(limits: &Limits, spec: &QuotaSpec, event: Value, bytes: u64, at: i64) -> bool {
    limits
        .quota(
            &org(),
            &pipeline(),
            "daily",
            spec,
            &event,
            bytes,
            TimestampMicros(at),
        )
        .await
}

fn key(day: i64) -> LimitKey {
    LimitKey {
        pipeline_id: pipeline(),
        step: "daily".into(),
        day,
    }
}

#[tokio::test]
async fn a_quota_lets_its_limit_through_and_counts_what_comes_past() {
    let limits = Limits::new(Arc::new(MemoryLimitUsageLedger::default()));
    let spec = quota(json!({
        "unit": "events", "limit": 2, "partitionBy": ["service"],
        "overrides": [{ "values": { "service": "checkout" }, "limit": 3 }],
    }));

    let mut cart = Vec::new();
    let mut checkout = Vec::new();

    for _ in 0..4 {
        cart.push(admits(&limits, &spec, json!({ "service": "cart" }), 10, 1).await);
        checkout.push(admits(&limits, &spec, json!({ "service": "checkout" }), 10, 1).await);
    }

    assert_eq!(cart, [true, true, false, false]);
    assert_eq!(checkout, [true, true, true, false]);

    let used = limits.quotas.used(&key(0), r#"["cart"]"#);
    assert_eq!(used.admitted_events, 2);
    assert_eq!(used.admitted_bytes, 20);
    assert_eq!(used.over_events, 2);
}

#[tokio::test]
async fn a_quota_in_bytes_lets_the_event_that_crosses_the_limit_through() {
    let limits = Limits::new(Arc::new(MemoryLimitUsageLedger::default()));
    let spec = quota(json!({ "unit": "bytes", "limit": 100 }));

    assert!(admits(&limits, &spec, json!({}), 60, 1).await);
    assert!(admits(&limits, &spec, json!({}), 60, 1).await);
    assert!(!admits(&limits, &spec, json!({}), 1, 1).await);
}

#[tokio::test]
async fn a_restart_goes_on_from_the_counts_of_the_day() {
    let ledger = Arc::new(MemoryLimitUsageLedger::default());
    let spec = quota(json!({ "unit": "events", "limit": 3 }));
    let before = Limits::new(ledger.clone());

    assert!(admits(&before, &spec, json!({}), 1, SECOND).await);
    assert!(admits(&before, &spec, json!({}), 1, SECOND).await);
    before.sync(TimestampMicros(2 * SECOND)).await.unwrap();

    let after = Limits::new(ledger.clone());

    assert!(admits(&after, &spec, json!({}), 1, 3 * SECOND).await);
    assert!(!admits(&after, &spec, json!({}), 1, 3 * SECOND).await);

    // A new UTC day starts from nothing.
    assert!(admits(&after, &spec, json!({}), 1, DAY_MICROS + SECOND).await);
}

#[tokio::test]
async fn nodes_see_each_others_counts_after_a_sync() {
    let ledger = Arc::new(MemoryLimitUsageLedger::default());
    let spec = quota(json!({ "unit": "events", "limit": 4 }));
    let one = Limits::new(ledger.clone());
    let other = Limits::new(ledger.clone());

    for _ in 0..3 {
        assert!(admits(&one, &spec, json!({}), 1, SECOND).await);
    }

    // Before the sync the other node sees only its own counts: this is the overshoot the docs name.
    assert!(admits(&other, &spec, json!({}), 1, SECOND).await);

    one.sync(TimestampMicros(SECOND)).await.unwrap();
    other.sync(TimestampMicros(SECOND)).await.unwrap();

    assert!(!admits(&other, &spec, json!({}), 1, 2 * SECOND).await);

    let stored = ledger.totals(&[key(0)]).await.unwrap();
    assert_eq!(stored[0].usage.admitted_events, 4);
    assert_eq!(stored[0].usage.over_events, 0);
}

/// A ledger that refuses the first write.
struct Flaky {
    inner: MemoryLimitUsageLedger,
    failed: AtomicBool,
}

#[async_trait]
impl LimitUsageLedger for Flaky {
    async fn add(&self, rows: &[LimitUsageRow], at: TimestampMicros) -> Result<()> {
        if !self.failed.swap(true, Ordering::SeqCst) {
            return Err(Error::unavailable("database down"));
        }

        self.inner.add(rows, at).await
    }

    async fn totals(&self, keys: &[LimitKey]) -> Result<Vec<LimitUsageRow>> {
        self.inner.totals(keys).await
    }

    async fn of_pipeline(&self, org: &Id, pipeline: &Id, day: i64) -> Result<Vec<LimitUsageRow>> {
        self.inner.of_pipeline(org, pipeline, day).await
    }

    async fn forget_before(&self, day: i64) -> Result<u64> {
        self.inner.forget_before(day).await
    }
}

#[tokio::test]
async fn counts_that_could_not_be_written_go_with_the_next_sync() {
    let ledger = Arc::new(Flaky {
        inner: MemoryLimitUsageLedger::default(),
        failed: AtomicBool::new(false),
    });
    let limits = Limits::new(ledger.clone());
    let spec = quota(json!({ "unit": "events", "limit": 2 }));
    let burst = throttle(json!({ "limit": 1, "windowSecs": 60 }));

    assert!(admits(&limits, &spec, json!({}), 1, SECOND).await);
    assert!(limits.throttle(
        &org(),
        &pipeline(),
        "burst",
        &burst,
        &json!({}),
        5,
        TimestampMicros(SECOND)
    ));

    assert!(limits.sync(TimestampMicros(SECOND)).await.is_err());

    // Still counted while waiting to be written.
    assert!(admits(&limits, &spec, json!({}), 1, SECOND).await);
    assert!(!admits(&limits, &spec, json!({}), 1, SECOND).await);

    limits.sync(TimestampMicros(2 * SECOND)).await.unwrap();

    let stored = ledger.of_pipeline(&org(), &pipeline(), 0).await.unwrap();
    let daily = stored.iter().find(|row| row.key.step == "daily").unwrap();
    let throttled = stored.iter().find(|row| row.key.step == "burst").unwrap();

    assert_eq!(daily.usage.admitted_events, 2);
    assert_eq!(daily.usage.over_events, 1);
    assert_eq!(throttled.usage.admitted_bytes, 5);
}

#[tokio::test]
async fn buckets_past_the_cap_share_one() {
    let limits = Limits::new(Arc::new(MemoryLimitUsageLedger::default()));
    let spec = quota(json!({
        "unit": "events", "limit": 1, "partitionBy": ["user"],
        "overrides": [{ "values": { "user": "vip" }, "limit": 2 }],
    }));

    for user in 0..MAX_QUOTA_BUCKETS {
        assert!(admits(&limits, &spec, json!({ "user": user }), 1, 1).await);
    }

    assert!(admits(&limits, &spec, json!({ "user": "late-1" }), 1, 1).await);
    assert!(!admits(&limits, &spec, json!({ "user": "late-2" }), 1, 1).await);
    assert_eq!(limits.quotas.used(&key(0), OTHER_BUCKET).admitted_events, 1);

    // A bucket with a limit of its own is always kept apart.
    assert!(admits(&limits, &spec, json!({ "user": "vip" }), 1, 1).await);
    assert!(admits(&limits, &spec, json!({ "user": "vip" }), 1, 1).await);
}

#[test]
fn a_throttle_lets_a_burst_through_then_one_per_share_of_the_window() {
    let limits = Limits::new(Arc::new(MemoryLimitUsageLedger::default()));
    let spec = throttle(json!({ "limit": 3, "windowSecs": 3, "groupBy": ["host"] }));
    let at = |host: &str, second: f64| {
        limits.throttle(
            &org(),
            &pipeline(),
            "burst",
            &spec,
            &json!({ "host": host }),
            1,
            TimestampMicros((second * SECOND as f64) as i64),
        )
    };

    assert_eq!(
        [at("a", 0.0), at("a", 0.0), at("a", 0.0), at("a", 0.0)],
        [true, true, true, false]
    );
    assert!(at("b", 0.0), "every group has its own");
    assert!(!at("a", 0.5));
    assert!(at("a", 1.0));
    assert!(!at("a", 1.0));
    assert!(at("a", 2.0));

    // After a quiet window the whole limit goes through again.
    assert_eq!(
        [at("a", 10.0), at("a", 10.0), at("a", 10.0), at("a", 10.0)],
        [true, true, true, false]
    );
}

#[test]
fn groups_past_the_cap_share_one() {
    let limits = Limits::new(Arc::new(MemoryLimitUsageLedger::default()));
    let spec = throttle(json!({ "limit": 1, "windowSecs": 60, "groupBy": ["user"] }));
    let admit = |user: Value| {
        limits.throttle(
            &org(),
            &pipeline(),
            "burst",
            &spec,
            &json!({ "user": user }),
            1,
            TimestampMicros(SECOND),
        )
    };

    for user in 0..MAX_THROTTLE_GROUPS {
        assert!(admit(json!(user)));
    }

    assert!(admit(json!("late-1")));
    assert!(!admit(json!("late-2")));
}

#[test]
fn changed_throttle_settings_start_over() {
    let limits = Limits::new(Arc::new(MemoryLimitUsageLedger::default()));
    let one = throttle(json!({ "limit": 1, "windowSecs": 60 }));
    let two = throttle(json!({ "limit": 2, "windowSecs": 60 }));
    let admit = |spec: &ThrottleSpec| {
        limits.throttle(
            &org(),
            &pipeline(),
            "burst",
            spec,
            &json!({}),
            1,
            TimestampMicros(SECOND),
        )
    };

    assert!(admit(&one));
    assert!(!admit(&one));
    assert!(admit(&two));
    assert!(admit(&two));
    assert!(!admit(&two));
}
