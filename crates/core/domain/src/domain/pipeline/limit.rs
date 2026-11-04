// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Steps of a realtime pipeline that let only so much through: a quota a day (`quota`) and a rate
//! (`throttle`). What goes past the limit is dropped, let through anyway, or sent to another stream,
//! as the step says. They limit only where someone put them into a pipeline: nothing else in the
//! platform drops or holds back data because of a limit.
//!
//! - **Quota:** counts events or bytes per UTC day, by arrival. Split by fields, every combination of
//!   their values is a bucket with its own count, and the limit of the step or one set for that
//!   bucket. The counts are stored ([`LimitUsageLedger`]), so a restart goes on from them; nodes add
//!   theirs every few seconds, so a quota can let a little more through than its limit.
//! - **Throttle:** lets at most `limit` events through per `window_secs` for each group, counted on
//!   each node by arrival. After a quiet window it lets `limit` through at once, then one every
//!   `window_secs / limit`.
use std::{
    collections::{BTreeMap, HashMap},
    sync::Mutex,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    condition::field_value,
    metric::{cut, label_value},
    realtime::writable_stream,
};
use crate::shared::{Error, Result, ids::Id, time::TimestampMicros};

pub const MAX_PARTITION_FIELDS: usize = 5;
/// At most this many buckets a quota keeps apart in a day; the events of the others share
/// [`OTHER_BUCKET`]. Buckets with a limit of their own are always kept apart.
pub const MAX_QUOTA_BUCKETS: usize = 1_000;
pub const MAX_OVERRIDES: usize = 100;
pub const OTHER_BUCKET: &str = "*";
pub const MAX_QUOTA_LIMIT: u64 = 1_000_000_000_000_000;
pub const MAX_THROTTLE_LIMIT: u32 = 1_000_000;
pub const MAX_THROTTLE_WINDOW_SECS: u32 = 86_400;
/// At most this many groups a throttle keeps apart on one node; the others share one.
pub const MAX_THROTTLE_GROUPS: usize = 10_000;
pub const DAY_MICROS: i64 = 86_400 * 1_000_000;
/// A value of a bucket or a group is cut to this many bytes.
const MAX_KEY_VALUE_LENGTH: usize = 128;
/// A bucket named by longer values than this, which only text full of escapes can make, counts in
/// [`OTHER_BUCKET`]: the ledger stores a bucket in at most 1024 characters.
const MAX_BUCKET_LENGTH: usize = 1_000;
const MAX_FIELD_LENGTH: usize = 255;

/// What a quota counts.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QuotaUnit {
    Events,
    /// The size of an event as it arrived, the way intake usage measures it.
    Bytes,
}

/// What happens to an event past the limit.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverAction {
    #[default]
    Drop,
    /// It goes on; the count says how much came past the limit. Quotas only.
    Pass,
    /// It leaves the pipeline at this step for `target`, as it is there.
    Route,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuotaSpec {
    pub unit: QuotaUnit,
    pub limit: u64,
    #[serde(default)]
    pub partition_by: Vec<String>,
    /// Buckets with a limit of their own.
    #[serde(default)]
    pub overrides: Vec<QuotaOverride>,
    #[serde(default)]
    pub action: OverAction,
    #[serde(default)]
    pub target: String,
    /// The limits of `overrides` by bucket, filled by [`QuotaSpec::prepare`].
    #[serde(skip)]
    limits: HashMap<String, u64>,
}

/// One bucket, named by the value of every field the quota is split by, and its limit.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct QuotaOverride {
    pub values: BTreeMap<String, String>,
    pub limit: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThrottleSpec {
    pub limit: u32,
    pub window_secs: u32,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default)]
    pub action: OverAction,
    #[serde(default)]
    pub target: String,
}

impl QuotaSpec {
    /// Checks the quota and reads its overrides into buckets.
    pub fn prepare(&mut self) -> Result<()> {
        check_limit(self.limit)?;
        check_fields(&self.partition_by, "a quota is split by")?;
        check_action(self.action, &self.target)?;

        if self.overrides.len() > MAX_OVERRIDES {
            return Err(Error::invalid(format!(
                "a quota has at most {MAX_OVERRIDES} buckets with a limit of their own"
            )));
        }

        if !self.overrides.is_empty() && self.partition_by.is_empty() {
            return Err(Error::invalid(
                "a bucket with a limit of its own needs a quota split by fields",
            ));
        }

        let mut limits = HashMap::new();

        for item in &self.overrides {
            check_limit(item.limit)?;

            let named = item.values.len() == self.partition_by.len()
                && self
                    .partition_by
                    .iter()
                    .all(|field| item.values.contains_key(field));

            if !named {
                return Err(Error::invalid(format!(
                    "a bucket with a limit of its own names a value for each of {}",
                    self.partition_by.join(", ")
                )));
            }

            let values = self.partition_by.iter().map(|field| {
                item.values
                    .get(field)
                    .map(|v| cut(v.clone(), MAX_KEY_VALUE_LENGTH))
            });

            if limits.insert(key_of(values), item.limit).is_some() {
                return Err(Error::invalid("a bucket has its own limit twice"));
            }
        }

        self.limits = limits;
        Ok(())
    }

    /// The bucket `event` counts in: empty when the quota is not split, else the values of the
    /// fields it is split by as a JSON list (`null` where the event has none).
    pub fn bucket(&self, event: &Value) -> String {
        if self.partition_by.is_empty() {
            return String::new();
        }

        let bucket = key_of(self.partition_by.iter().map(|field| value_at(event, field)));

        if bucket.len() > MAX_BUCKET_LENGTH {
            OTHER_BUCKET.to_owned()
        } else {
            bucket
        }
    }

    /// The limit of a bucket: its own, or the quota's.
    pub fn limit_of(&self, bucket: &str) -> u64 {
        self.limits.get(bucket).copied().unwrap_or(self.limit)
    }

    /// Whether a bucket has a limit of its own, which keeps it apart past [`MAX_QUOTA_BUCKETS`].
    pub fn has_own_limit(&self, bucket: &str) -> bool {
        self.limits.contains_key(bucket)
    }

    /// What an event counts for in this quota's unit.
    pub fn units(&self, bytes: u64) -> u64 {
        match self.unit {
            QuotaUnit::Events => 1,
            QuotaUnit::Bytes => bytes,
        }
    }
}

impl ThrottleSpec {
    pub fn validate(&self) -> Result<()> {
        if self.limit == 0 || self.limit > MAX_THROTTLE_LIMIT {
            return Err(Error::invalid(format!(
                "a throttle lets 1 to {MAX_THROTTLE_LIMIT} events through per window"
            )));
        }

        if self.window_secs == 0 || self.window_secs > MAX_THROTTLE_WINDOW_SECS {
            return Err(Error::invalid(format!(
                "a throttle's window is 1 to {MAX_THROTTLE_WINDOW_SECS} seconds"
            )));
        }

        if self.action == OverAction::Pass {
            return Err(Error::invalid(
                "a throttle drops what goes past it or sends it to another stream",
            ));
        }

        check_fields(&self.group_by, "a throttle groups by")?;
        check_action(self.action, &self.target)
    }

    /// The group `event` is throttled in, made like a quota's bucket.
    pub fn group(&self, event: &Value) -> String {
        if self.group_by.is_empty() {
            return String::new();
        }

        key_of(self.group_by.iter().map(|field| value_at(event, field)))
    }
}

fn check_limit(limit: u64) -> Result<()> {
    if limit == 0 || limit > MAX_QUOTA_LIMIT {
        return Err(Error::invalid(format!(
            "a quota's limit is a whole number from 1 to {MAX_QUOTA_LIMIT}"
        )));
    }

    Ok(())
}

fn check_fields(fields: &[String], what: &str) -> Result<()> {
    if fields.len() > MAX_PARTITION_FIELDS {
        return Err(Error::invalid(format!(
            "{what} at most {MAX_PARTITION_FIELDS} fields"
        )));
    }

    for (index, field) in fields.iter().enumerate() {
        if field.trim().is_empty() || field.trim() != field || field.len() > MAX_FIELD_LENGTH {
            return Err(Error::invalid(format!("{what} a field that has no name")));
        }

        if fields[..index].contains(field) {
            return Err(Error::invalid(format!("{what} `{field}` twice")));
        }
    }

    Ok(())
}

fn check_action(action: OverAction, target: &str) -> Result<()> {
    if action == OverAction::Route {
        if target.trim().is_empty() {
            return Err(Error::invalid(
                "name the stream that what goes past the limit is sent to",
            ));
        }

        writable_stream(target)?;
    }

    Ok(())
}

fn value_at(event: &Value, field: &str) -> Option<String> {
    field_value(event, field)
        .and_then(label_value)
        .map(|value| cut(value, MAX_KEY_VALUE_LENGTH))
}

fn key_of(values: impl Iterator<Item = Option<String>>) -> String {
    Value::Array(
        values
            .map(|value| value.map_or(Value::Null, Value::String))
            .collect(),
    )
    .to_string()
}

/// The start of the UTC day of `at`.
pub fn utc_day(at: TimestampMicros) -> i64 {
    at.0.div_euclid(DAY_MICROS) * DAY_MICROS
}

/// What a limit step let through and what came past it, in events and bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitUsage {
    pub admitted_events: u64,
    pub admitted_bytes: u64,
    pub over_events: u64,
    pub over_bytes: u64,
}

impl LimitUsage {
    pub fn add(&mut self, other: &Self) {
        self.admitted_events = self.admitted_events.saturating_add(other.admitted_events);
        self.admitted_bytes = self.admitted_bytes.saturating_add(other.admitted_bytes);
        self.over_events = self.over_events.saturating_add(other.over_events);
        self.over_bytes = self.over_bytes.saturating_add(other.over_bytes);
    }

    /// What was let through, in `unit`.
    pub fn admitted(&self, unit: QuotaUnit) -> u64 {
        match unit {
            QuotaUnit::Events => self.admitted_events,
            QuotaUnit::Bytes => self.admitted_bytes,
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// One step of one pipeline on one UTC day.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LimitKey {
    pub pipeline_id: Id,
    /// The step's name, unique among a pipeline's limit steps.
    pub step: String,
    /// The start of the UTC day.
    pub day: i64,
}

/// The usage of one bucket of a step on a day. A throttle has one bucket, the empty one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitUsageRow {
    pub org_id: Id,
    pub key: LimitKey,
    pub bucket: String,
    pub usage: LimitUsage,
}

/// Where the counts of limit steps are kept, for every node and across restarts.
#[async_trait]
pub trait LimitUsageLedger: Send + Sync {
    /// Adds what this node counted since it last wrote. Each (key, bucket) appears once.
    async fn add(&self, rows: &[LimitUsageRow], at: TimestampMicros) -> Result<()>;

    /// Every bucket stored under these keys, with what all nodes added.
    async fn totals(&self, keys: &[LimitKey]) -> Result<Vec<LimitUsageRow>>;

    /// The buckets of one pipeline on one day, for the organization that owns it.
    async fn of_pipeline(&self, org: &Id, pipeline: &Id, day: i64) -> Result<Vec<LimitUsageRow>>;

    /// Forgets the days before `day`.
    async fn forget_before(&self, day: i64) -> Result<u64>;
}

/// A ledger in memory, for tests.
#[derive(Default)]
pub struct MemoryLimitUsageLedger {
    rows: Mutex<Vec<LimitUsageRow>>,
}

#[async_trait]
impl LimitUsageLedger for MemoryLimitUsageLedger {
    async fn add(&self, rows: &[LimitUsageRow], _: TimestampMicros) -> Result<()> {
        let mut stored = self.rows.lock().expect("limit usage lock");

        for row in rows {
            match stored
                .iter_mut()
                .find(|item| item.key == row.key && item.bucket == row.bucket)
            {
                Some(item) => item.usage.add(&row.usage),
                None => stored.push(row.clone()),
            }
        }

        Ok(())
    }

    async fn totals(&self, keys: &[LimitKey]) -> Result<Vec<LimitUsageRow>> {
        Ok(self
            .rows
            .lock()
            .expect("limit usage lock")
            .iter()
            .filter(|row| keys.contains(&row.key))
            .cloned()
            .collect())
    }

    async fn of_pipeline(&self, org: &Id, pipeline: &Id, day: i64) -> Result<Vec<LimitUsageRow>> {
        Ok(self
            .rows
            .lock()
            .expect("limit usage lock")
            .iter()
            .filter(|row| {
                &row.org_id == org && &row.key.pipeline_id == pipeline && row.key.day == day
            })
            .cloned()
            .collect())
    }

    async fn forget_before(&self, day: i64) -> Result<u64> {
        let mut rows = self.rows.lock().expect("limit usage lock");
        let before = rows.len();

        rows.retain(|row| row.key.day >= day);
        Ok((before - rows.len()) as u64)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn quota(value: Value) -> Result<QuotaSpec> {
        let mut spec: QuotaSpec =
            serde_json::from_value(value).map_err(|e| Error::invalid(e.to_string()))?;
        spec.prepare().map(|()| spec)
    }

    #[test]
    fn a_split_quota_counts_each_combination_apart_with_its_own_limits() {
        let spec = quota(json!({
            "unit": "events",
            "limit": 1000,
            "partitionBy": ["service", "env"],
            "overrides": [{ "values": { "service": "checkout", "env": "prod" }, "limit": 5000 }],
        }))
        .unwrap();

        let checkout = spec.bucket(&json!({ "service": "checkout", "env": "prod" }));
        let cart = spec.bucket(&json!({ "service": "cart" }));

        assert_eq!(checkout, r#"["checkout","prod"]"#);
        assert_eq!(cart, r#"["cart",null]"#);
        assert_eq!(spec.limit_of(&checkout), 5000);
        assert!(spec.has_own_limit(&checkout));
        assert_eq!(spec.limit_of(&cart), 1000);
        assert_eq!(spec.units(512), 1);
        assert_eq!(spec.action, OverAction::Drop);

        let whole = quota(json!({ "unit": "bytes", "limit": 1 })).unwrap();
        assert_eq!(whole.bucket(&json!({ "service": "x" })), "");
        assert_eq!(whole.units(512), 512);
    }

    #[test]
    fn a_quota_that_cannot_be_kept_is_refused() {
        for wrong in [
            json!({ "unit": "events", "limit": 0 }),
            json!({ "unit": "rows", "limit": 1 }),
            json!({ "unit": "events", "limit": 1, "action": "route" }),
            json!({ "unit": "events", "limit": 1, "action": "route", "target": "_audit" }),
            json!({ "unit": "events", "limit": 1, "partitionBy": ["a", "a"] }),
            json!({ "unit": "events", "limit": 1, "partitionBy": ["a", "b", "c", "d", "e", "f"] }),
            json!({ "unit": "events", "limit": 1, "overrides": [{ "values": {}, "limit": 2 }] }),
            json!({
                "unit": "events", "limit": 1, "partitionBy": ["a", "b"],
                "overrides": [{ "values": { "a": "x" }, "limit": 2 }],
            }),
            json!({
                "unit": "events", "limit": 1, "partitionBy": ["a"],
                "overrides": [
                    { "values": { "a": "x" }, "limit": 2 },
                    { "values": { "a": "x" }, "limit": 3 },
                ],
            }),
        ] {
            assert!(quota(wrong.clone()).is_err(), "{wrong}");
        }

        let routed = quota(json!({
            "unit": "events", "limit": 1, "action": "route", "target": "logs_cold",
        }))
        .unwrap();
        assert_eq!(routed.action, OverAction::Route);
    }

    #[test]
    fn a_throttle_drops_or_sends_elsewhere() {
        let spec = |value: Value| {
            serde_json::from_value::<ThrottleSpec>(value)
                .map_err(|e| Error::invalid(e.to_string()))
                .and_then(|spec| spec.validate().map(|()| spec))
        };

        let per_host =
            spec(json!({ "limit": 100, "windowSecs": 60, "groupBy": ["host"] })).unwrap();
        assert_eq!(per_host.group(&json!({ "host": "web-1" })), r#"["web-1"]"#);

        for wrong in [
            json!({ "limit": 0, "windowSecs": 60 }),
            json!({ "limit": 10, "windowSecs": 0 }),
            json!({ "limit": 10, "windowSecs": 86401 }),
            json!({ "limit": 10, "windowSecs": 60, "action": "pass" }),
            json!({ "limit": 10, "windowSecs": 60, "action": "route" }),
        ] {
            assert!(spec(wrong.clone()).is_err(), "{wrong}");
        }
    }

    #[tokio::test]
    async fn the_memory_ledger_adds_up_and_forgets_old_days() {
        let ledger = MemoryLimitUsageLedger::default();
        let key = |day| LimitKey {
            pipeline_id: Id("p".into()),
            step: "quota".into(),
            day,
        };
        let row = |day, events| LimitUsageRow {
            org_id: Id("org".into()),
            key: key(day),
            bucket: String::new(),
            usage: LimitUsage {
                admitted_events: events,
                ..LimitUsage::default()
            },
        };

        ledger
            .add(&[row(0, 2), row(DAY_MICROS, 1)], TimestampMicros(1))
            .await
            .unwrap();
        ledger.add(&[row(0, 3)], TimestampMicros(2)).await.unwrap();

        let totals = ledger.totals(&[key(0)]).await.unwrap();
        assert_eq!(totals.len(), 1);
        assert_eq!(totals[0].usage.admitted_events, 5);

        assert_eq!(ledger.forget_before(DAY_MICROS).await.unwrap(), 1);
        assert_eq!(utc_day(TimestampMicros(DAY_MICROS + 5)), DAY_MICROS);
    }
}
