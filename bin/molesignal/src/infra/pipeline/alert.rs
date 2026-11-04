// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Alert pipelines in `scheduled_pipelines` (`function_steps.mode = "alert"`).
//!
//! - An organization's enabled alert pipelines are read at once and kept for [`TTL`], as the
//!   realtime ones are: another node's change shows within that time, this node's at once
//!   ([`super::note_change`]).
//! - What became of the events is counted in the pipeline's `run_state` (`alert`): processed,
//!   dropped and failed today (UTC), with the latest drop and the latest failure.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde_json::{Value, json};
use sqlx::{PgPool, Row, types::Json};

use crate::{
    domain::pipeline::alert::{
        AlertConfig, AlertOutcome, AlertOutcomeKind, AlertPipeline, AlertPipelineRepository,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// How long an organization's alert pipelines stay cached.
const TTL: Duration = Duration::from_secs(10);

struct Loaded {
    at: Instant,
    changes: u64,
    pipelines: Arc<Vec<AlertPipeline>>,
}

pub struct PgAlertPipelineRepository {
    pool: PgPool,
    loaded: Mutex<HashMap<Id, Loaded>>,
}

impl PgAlertPipelineRepository {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            loaded: Mutex::new(HashMap::new()),
        }
    }

    async fn load(&self, org: &Id) -> Result<Vec<AlertPipeline>> {
        let rows = sqlx::query("SELECT id, name, function_steps, updated_at FROM scheduled_pipelines WHERE org_id = $1 AND enabled = TRUE AND function_steps->>'mode' = 'alert'")
            .bind(org.as_str()).fetch_all(&self.pool).await.map_err(crate::infra::persistence::sqlx_err)?;

        let mut pipelines: Vec<AlertPipeline> = rows
            .into_iter()
            .filter_map(|row| {
                let id = Id(row.get("id"));
                let config: Json<Value> = row.get("function_steps");

                match AlertConfig::parse(&config.0) {
                    Ok(config) => Some(AlertPipeline {
                        id,
                        org_id: org.clone(),
                        name: row.get("name"),
                        config,
                        updated_at: row.get::<TimestampMicros, _>("updated_at"),
                    }),
                    Err(error) => {
                        tracing::warn!(pipeline_id = %id, %error, "alert pipeline cannot be read; skipped");
                        None
                    }
                }
            })
            .collect();

        pipelines.sort_by(|a, b| {
            a.config
                .order
                .cmp(&b.config.order)
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(pipelines)
    }
}

#[async_trait]
impl AlertPipelineRepository for PgAlertPipelineRepository {
    async fn enabled(&self, org: &Id) -> Result<Vec<AlertPipeline>> {
        let changes = super::changes();

        let cached = self.loaded.lock().ok().and_then(|loaded| {
            loaded
                .get(org)
                .filter(|entry| entry.changes == changes && entry.at.elapsed() < TTL)
                .map(|entry| entry.pipelines.clone())
        });

        if let Some(pipelines) = cached {
            return Ok(pipelines.as_ref().clone());
        }

        let pipelines = Arc::new(self.load(org).await?);

        if let Ok(mut loaded) = self.loaded.lock() {
            loaded.insert(
                org.clone(),
                Loaded {
                    at: Instant::now(),
                    changes,
                    pipelines: pipelines.clone(),
                },
            );
        }

        Ok(pipelines.as_ref().clone())
    }

    async fn record(&self, pipeline: &Id, outcome: AlertOutcome) -> Result<()> {
        let (counter, latest) = match outcome.kind {
            AlertOutcomeKind::Processed => ("processed", None),
            AlertOutcomeKind::Dropped => ("dropped", Some("last_dropped")),
            AlertOutcomeKind::Failed => ("failed", Some("last_failed")),
        };

        let latest = latest.map(|key| {
            let mut field = serde_json::Map::new();

            field.insert(
                key.to_owned(),
                json!({
                    "at_time": outcome.at.0,
                    "rule": outcome.rule_name,
                    "reason": outcome.reason,
                }),
            );

            Value::Object(field)
        });

        // The counts are of one UTC day: a record on another day starts them again.
        sqlx::query(
            "UPDATE scheduled_pipelines
                SET run_state = jsonb_build_object('alert',
                    CASE WHEN run_state->'alert'->>'day' = $2 THEN run_state->'alert'
                         ELSE jsonb_build_object('day', $2) END
                    || jsonb_build_object($3::text,
                        COALESCE((CASE WHEN run_state->'alert'->>'day' = $2
                                       THEN run_state->'alert'->>$3 END)::bigint, 0) + 1)
                    || COALESCE($4::jsonb, '{}'::jsonb))
              WHERE id = $1",
        )
        .bind(pipeline.as_str())
        .bind(day_of(outcome.at))
        .bind(counter)
        .bind(latest.map(Json))
        .execute(&self.pool)
        .await
        .map_err(crate::infra::persistence::sqlx_err)?;

        Ok(())
    }
}

/// The UTC day of a moment, as `YYYY-MM-DD`.
fn day_of(at: TimestampMicros) -> String {
    chrono::DateTime::from_timestamp_micros(at.0)
        .map(|moment| moment.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_kept_per_utc_day() {
        assert_eq!(day_of(TimestampMicros(0)), "1970-01-01");
        assert_eq!(
            day_of(TimestampMicros(1_761_696_000_000_000 - 1)),
            "2025-10-28"
        );
    }
}
