// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Quotas and throttles as events arrive ([`crate::domain::pipeline::limit`]).
//!
//! Every few seconds a node writes what it counted into the ledger and reads back what all nodes
//! counted for its quotas ([`Limits::sync`]): between two syncs a quota sees only this node's new
//! counts, which is how much it can go past its limit when several nodes take its events. Throttles
//! decide on each node alone; the ledger keeps only what they let through and held back each day,
//! for people to read.
use std::{collections::HashMap, sync::Arc};

use parking_lot::Mutex;
use serde_json::Value;

use crate::{
    domain::pipeline::limit::{
        LimitKey, LimitUsage, LimitUsageLedger, LimitUsageRow, QuotaSpec, ThrottleSpec, utc_day,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

mod quota;
mod throttle;

use quota::QuotaBook;
use throttle::ThrottleBook;

/// A quota or a throttle no event has reached for this long is forgotten.
const IDLE_MICROS: i64 = 3_600 * 1_000_000;

pub struct Limits {
    ledger: Arc<dyn LimitUsageLedger>,
    quotas: QuotaBook,
    throttles: ThrottleBook,
    /// What each throttle let through and held back since the last sync.
    throttled: Mutex<HashMap<LimitKey, (Id, LimitUsage)>>,
}

impl Limits {
    pub fn new(ledger: Arc<dyn LimitUsageLedger>) -> Self {
        Self {
            ledger,
            quotas: QuotaBook::default(),
            throttles: ThrottleBook::default(),
            throttled: Mutex::new(HashMap::new()),
        }
    }

    pub fn ledger(&self) -> &Arc<dyn LimitUsageLedger> {
        &self.ledger
    }

    /// Whether the quota of a step lets `event`, of `bytes` as it arrived, through today.
    #[allow(clippy::too_many_arguments)]
    pub async fn quota(
        &self,
        org: &Id,
        pipeline: &Id,
        step: &str,
        spec: &QuotaSpec,
        event: &Value,
        bytes: u64,
        now: TimestampMicros,
    ) -> bool {
        let key = LimitKey {
            pipeline_id: pipeline.clone(),
            step: step.to_owned(),
            day: utc_day(now),
        };

        self.quotas
            .load(self.ledger.as_ref(), org, &key, now.0)
            .await;

        self.quotas
            .admit(spec, org, &key, spec.bucket(event), bytes, now.0)
    }

    /// Whether the throttle of a step lets `event` through now.
    #[allow(clippy::too_many_arguments)]
    pub fn throttle(
        &self,
        org: &Id,
        pipeline: &Id,
        step: &str,
        spec: &ThrottleSpec,
        event: &Value,
        bytes: u64,
        now: TimestampMicros,
    ) -> bool {
        let within = self
            .throttles
            .admit(pipeline, step, spec, spec.group(event), now.0);

        let key = LimitKey {
            pipeline_id: pipeline.clone(),
            step: step.to_owned(),
            day: utc_day(now),
        };

        let counted = if within {
            LimitUsage {
                admitted_events: 1,
                admitted_bytes: bytes,
                ..LimitUsage::default()
            }
        } else {
            LimitUsage {
                over_events: 1,
                over_bytes: bytes,
                ..LimitUsage::default()
            }
        };

        self.throttled
            .lock()
            .entry(key)
            .or_insert_with(|| (org.clone(), LimitUsage::default()))
            .1
            .add(&counted);

        within
    }

    /// Writes what this node counted since the last sync, then reads back its quotas' counts for
    /// every node. What could not be written is tried again with the next sync.
    pub async fn sync(&self, now: TimestampMicros) -> Result<()> {
        let (mut rows, keys) = self.quotas.begin_sync();
        let throttled: Vec<_> = self.throttled.lock().drain().collect();

        rows.extend(throttled.iter().map(|(key, (org, usage))| LimitUsageRow {
            org_id: org.clone(),
            key: key.clone(),
            bucket: String::new(),
            usage: *usage,
        }));

        if !rows.is_empty() {
            let written = self.ledger.add(&rows, now).await;
            self.quotas.end_write(written.is_ok());

            if let Err(error) = written {
                let mut pending = self.throttled.lock();

                for (key, (org, usage)) in throttled {
                    pending
                        .entry(key)
                        .or_insert_with(|| (org, LimitUsage::default()))
                        .1
                        .add(&usage);
                }

                return Err(error);
            }
        }

        if !keys.is_empty() {
            self.quotas.read_back(self.ledger.totals(&keys).await?);
        }

        self.quotas.forget(utc_day(now), now.0, IDLE_MICROS);
        self.throttles.forget(now.0, IDLE_MICROS);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
