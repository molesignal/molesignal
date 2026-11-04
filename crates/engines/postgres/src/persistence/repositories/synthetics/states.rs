// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use async_trait::async_trait;
use sqlx::Row;

use super::PgSyntheticRepository;
use crate::{
    domain::synthetics::{
        LocationStateUpdate, MissedRun, MonitorLocationState, MonitorState, StateObservation,
        SyntheticStateRepository, SyntheticStateTransition,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

const LOCATION_STATE_COLS: &str = "organization_id, monitor_id, monitor_revision_id, location_id,
    current_state, candidate_state, candidate_count, consecutive_missing, last_result_id,
    last_observed_at, updated_at";

const TRANSITION_COLS: &str = "id, organization_id, monitor_id, monitor_revision_id,
    location_id, previous_state, current_state, correlation_key, result_id,
    started_at, ended_at, created_at";

fn parse_state(value: &str) -> Result<MonitorState> {
    MonitorState::parse(value)
        .ok_or_else(|| Error::internal(format!("unknown synthetic state: {value}")))
}

fn next_location_state(
    previous: MonitorState,
    old_candidate: Option<MonitorState>,
    old_count: u32,
    observation: &StateObservation,
) -> (MonitorState, Option<MonitorState>, u32) {
    if observation.observed_state == previous {
        return (previous, None, 0);
    }

    // Unknown is not a healthy/failing baseline. The first known observation establishes
    // that baseline immediately; hysteresis applies only to transitions between known states.
    if previous == MonitorState::Unknown && observation.observed_state != MonitorState::Unknown {
        return (observation.observed_state, None, 0);
    }

    let count = if old_candidate == Some(observation.observed_state) {
        old_count.saturating_add(1)
    } else {
        1
    };

    let threshold = if observation.observed_state == MonitorState::Healthy {
        observation.recovery_threshold.max(1)
    } else {
        observation.failure_threshold.max(1)
    };

    if count >= threshold {
        (observation.observed_state, None, 0)
    } else {
        (previous, Some(observation.observed_state), count)
    }
}

fn row_to_location_state(row: sqlx::postgres::PgRow) -> Result<MonitorLocationState> {
    let current: String = row.try_get("current_state").map_err(super::sqlx_err)?;
    let candidate: Option<String> = row.try_get("candidate_state").map_err(super::sqlx_err)?;

    Ok(MonitorLocationState {
        organization_id: Id(row.try_get("organization_id").map_err(super::sqlx_err)?),
        monitor_id: Id(row.try_get("monitor_id").map_err(super::sqlx_err)?),
        monitor_revision_id: Id(row
            .try_get("monitor_revision_id")
            .map_err(super::sqlx_err)?),
        location_id: Id(row.try_get("location_id").map_err(super::sqlx_err)?),
        current_state: parse_state(&current)?,
        candidate_state: candidate.as_deref().map(parse_state).transpose()?,
        candidate_count: row
            .try_get::<i32, _>("candidate_count")
            .map_err(super::sqlx_err)? as u32,
        consecutive_missing: row
            .try_get::<i32, _>("consecutive_missing")
            .map_err(super::sqlx_err)? as u32,
        last_result_id: row
            .try_get::<Option<String>, _>("last_result_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        last_observed_at: row
            .try_get::<Option<TimestampMicros>, _>("last_observed_at")
            .map_err(super::sqlx_err)?,
        updated_at: row
            .try_get::<TimestampMicros, _>("updated_at")
            .map_err(super::sqlx_err)?,
    })
}

fn row_to_transition(row: sqlx::postgres::PgRow) -> Result<SyntheticStateTransition> {
    let previous: String = row.try_get("previous_state").map_err(super::sqlx_err)?;
    let current: String = row.try_get("current_state").map_err(super::sqlx_err)?;

    Ok(SyntheticStateTransition {
        id: Id(row.try_get("id").map_err(super::sqlx_err)?),
        organization_id: Id(row.try_get("organization_id").map_err(super::sqlx_err)?),
        monitor_id: Id(row.try_get("monitor_id").map_err(super::sqlx_err)?),
        monitor_revision_id: Id(row
            .try_get("monitor_revision_id")
            .map_err(super::sqlx_err)?),
        location_id: row
            .try_get::<Option<String>, _>("location_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        previous_state: parse_state(&previous)?,
        current_state: parse_state(&current)?,
        correlation_key: row.try_get("correlation_key").map_err(super::sqlx_err)?,
        result_id: row
            .try_get::<Option<String>, _>("result_id")
            .map_err(super::sqlx_err)?
            .map(Id),
        started_at: row
            .try_get::<TimestampMicros, _>("started_at")
            .map_err(super::sqlx_err)?,
        ended_at: row
            .try_get::<Option<TimestampMicros>, _>("ended_at")
            .map_err(super::sqlx_err)?,
        created_at: row
            .try_get::<TimestampMicros, _>("created_at")
            .map_err(super::sqlx_err)?,
    })
}

async fn insert_transition(
    connection: &mut sqlx::PgConnection,
    transition: &SyntheticStateTransition,
) -> Result<SyntheticStateTransition> {
    sqlx::query(
        "UPDATE synthetic_state_transitions
         SET ended_at = $4
         WHERE organization_id = $1 AND monitor_id = $2
           AND location_id IS NOT DISTINCT FROM $3 AND ended_at IS NULL",
    )
    .bind(&transition.organization_id.0)
    .bind(&transition.monitor_id.0)
    .bind(transition.location_id.as_ref().map(Id::as_str))
    .bind(transition.started_at)
    .execute(&mut *connection)
    .await
    .map_err(super::sqlx_err)?;

    let sql = format!(
        "INSERT INTO synthetic_state_transitions
            (id, organization_id, monitor_id, monitor_revision_id, location_id,
             previous_state, current_state, correlation_key, result_id,
             started_at, ended_at, created_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NULL, $11)
         RETURNING {TRANSITION_COLS}"
    );

    let row = sqlx::query(&sql)
        .bind(&transition.id.0)
        .bind(&transition.organization_id.0)
        .bind(&transition.monitor_id.0)
        .bind(&transition.monitor_revision_id.0)
        .bind(transition.location_id.as_ref().map(Id::as_str))
        .bind(transition.previous_state.as_str())
        .bind(transition.current_state.as_str())
        .bind(&transition.correlation_key)
        .bind(transition.result_id.as_ref().map(Id::as_str))
        .bind(transition.started_at)
        .bind(transition.created_at)
        .fetch_one(&mut *connection)
        .await
        .map_err(super::sqlx_err)?;

    row_to_transition(row)
}

#[async_trait]
impl SyntheticStateRepository for PgSyntheticRepository {
    async fn apply_location_observation(
        &self,
        observation: StateObservation,
    ) -> Result<LocationStateUpdate> {
        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;

        let sql = format!(
            "SELECT {LOCATION_STATE_COLS}
             FROM synthetic_monitor_location_states
             WHERE organization_id = $1 AND monitor_id = $2
               AND monitor_revision_id = $3 AND location_id = $4
             FOR UPDATE"
        );

        let existing = sqlx::query(&sql)
            .bind(&observation.organization_id.0)
            .bind(&observation.monitor_id.0)
            .bind(&observation.monitor_revision_id.0)
            .bind(&observation.location_id.0)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?
            .map(row_to_location_state)
            .transpose()?;

        let previous = existing
            .as_ref()
            .map(|state| state.current_state)
            .unwrap_or(MonitorState::Unknown);

        let old_candidate = existing.as_ref().and_then(|state| state.candidate_state);

        let old_count = existing
            .as_ref()
            .map(|state| state.candidate_count)
            .unwrap_or(0);

        let previous_missing = existing.as_ref().map(|state| state.consecutive_missing);

        let (current, candidate, count) =
            next_location_state(previous, old_candidate, old_count, &observation);

        // A run the location made ends a stretch of missed ones.
        let sql = format!(
            "INSERT INTO synthetic_monitor_location_states
                (organization_id, monitor_id, monitor_revision_id, location_id,
                 current_state, candidate_state, candidate_count, consecutive_missing,
                 last_result_id, last_observed_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, 0, $8, $9, $9)
             ON CONFLICT (organization_id, monitor_id, monitor_revision_id, location_id)
             DO UPDATE SET current_state = EXCLUDED.current_state,
                 candidate_state = EXCLUDED.candidate_state,
                 candidate_count = EXCLUDED.candidate_count,
                 consecutive_missing = 0,
                 last_result_id = EXCLUDED.last_result_id,
                 last_observed_at = EXCLUDED.last_observed_at,
                 updated_at = EXCLUDED.updated_at
             RETURNING {LOCATION_STATE_COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&observation.organization_id.0)
            .bind(&observation.monitor_id.0)
            .bind(&observation.monitor_revision_id.0)
            .bind(&observation.location_id.0)
            .bind(current.as_str())
            .bind(candidate.map(MonitorState::as_str))
            .bind(count as i32)
            .bind(&observation.result_id.0)
            .bind(observation.observed_at)
            .fetch_one(&mut *transaction)
            .await
            .map_err(super::sqlx_err)?;

        let state = row_to_location_state(row)?;

        let transition = if current != previous {
            Some(
                insert_transition(
                    &mut transaction,
                    &SyntheticStateTransition {
                        id: Id::new(),
                        organization_id: observation.organization_id.clone(),
                        monitor_id: observation.monitor_id.clone(),
                        monitor_revision_id: observation.monitor_revision_id.clone(),
                        location_id: Some(observation.location_id.clone()),
                        previous_state: previous,
                        current_state: current,
                        correlation_key: format!(
                            "synthetic:{}:{}",
                            observation.monitor_id, observation.location_id
                        ),
                        result_id: Some(observation.result_id.clone()),
                        started_at: observation.observed_at,
                        ended_at: None,
                        created_at: observation.observed_at,
                    },
                )
                .await?,
            )
        } else {
            None
        };

        transaction.commit().await.map_err(super::sqlx_err)?;

        Ok(LocationStateUpdate {
            state,
            transition,
            previous_missing,
        })
    }

    async fn record_missed_run(&self, missed: MissedRun) -> Result<MonitorLocationState> {
        // The state stays what the location's last run made it; only the count moves.
        let sql = format!(
            "INSERT INTO synthetic_monitor_location_states
                (organization_id, monitor_id, monitor_revision_id, location_id,
                 consecutive_missing, updated_at)
             VALUES ($1, $2, $3, $4, 1, $5)
             ON CONFLICT (organization_id, monitor_id, monitor_revision_id, location_id)
             DO UPDATE SET
                 consecutive_missing = synthetic_monitor_location_states.consecutive_missing + 1,
                 updated_at = EXCLUDED.updated_at
             RETURNING {LOCATION_STATE_COLS}"
        );

        let row = sqlx::query(&sql)
            .bind(&missed.organization_id.0)
            .bind(&missed.monitor_id.0)
            .bind(&missed.monitor_revision_id.0)
            .bind(&missed.location_id.0)
            .bind(missed.missed_at)
            .fetch_one(&self.pool)
            .await
            .map_err(super::sqlx_err)?;

        row_to_location_state(row)
    }

    async fn list_location_states(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        revision_id: &Id,
    ) -> Result<Vec<MonitorLocationState>> {
        let sql = format!(
            "SELECT {LOCATION_STATE_COLS}
             FROM synthetic_monitor_location_states
             WHERE organization_id = $1 AND monitor_id = $2 AND monitor_revision_id = $3
             ORDER BY location_id"
        );

        sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&monitor_id.0)
            .bind(&revision_id.0)
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_location_state)
            .collect()
    }

    async fn record_state_transition(
        &self,
        transition: SyntheticStateTransition,
    ) -> Result<SyntheticStateTransition> {
        if transition.previous_state == transition.current_state {
            return Err(Error::invalid("state transition must change state"));
        }

        let mut transaction = sqlx::begin(&self.pool).await.map_err(super::sqlx_err)?;
        let transition = insert_transition(&mut transaction, &transition).await?;
        transaction.commit().await.map_err(super::sqlx_err)?;
        Ok(transition)
    }

    async fn list_state_transitions(
        &self,
        org_id: &Id,
        monitor_id: &Id,
        before: Option<TimestampMicros>,
        limit: u32,
    ) -> Result<Vec<SyntheticStateTransition>> {
        let sql = format!(
            "SELECT {TRANSITION_COLS} FROM synthetic_state_transitions
             WHERE organization_id = $1 AND monitor_id = $2
               AND ($3::TIMESTAMPTZ IS NULL OR started_at < $3)
             ORDER BY started_at DESC, id DESC LIMIT $4"
        );

        sqlx::query(&sql)
            .bind(&org_id.0)
            .bind(&monitor_id.0)
            .bind(before)
            .bind(i64::from(limit.clamp(1, 500)))
            .fetch_all(&self.pool)
            .await
            .map_err(super::sqlx_err)?
            .into_iter()
            .map(row_to_transition)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::next_location_state;
    use crate::{
        domain::synthetics::{MonitorState, StateObservation},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn observation(observed_state: MonitorState) -> StateObservation {
        StateObservation {
            organization_id: Id("org-a".into()),
            monitor_id: Id("monitor-a".into()),
            monitor_revision_id: Id("revision-a".into()),
            location_id: Id("location-a".into()),
            observed_state,
            result_id: Id("result-a".into()),
            observed_at: TimestampMicros(1),
            failure_threshold: 3,
            recovery_threshold: 2,
        }
    }

    #[test]
    fn first_known_observation_establishes_the_location_baseline() {
        let next = next_location_state(
            MonitorState::Unknown,
            None,
            0,
            &observation(MonitorState::Healthy),
        );

        assert_eq!(next, (MonitorState::Healthy, None, 0));
    }

    #[test]
    fn hysteresis_still_applies_between_known_states() {
        let failing = observation(MonitorState::Failing);
        assert_eq!(
            next_location_state(MonitorState::Healthy, None, 0, &failing),
            (MonitorState::Healthy, Some(MonitorState::Failing), 1)
        );
        assert_eq!(
            next_location_state(
                MonitorState::Healthy,
                Some(MonitorState::Failing),
                1,
                &failing,
            ),
            (MonitorState::Healthy, Some(MonitorState::Failing), 2)
        );
        assert_eq!(
            next_location_state(
                MonitorState::Healthy,
                Some(MonitorState::Failing),
                2,
                &failing,
            ),
            (MonitorState::Failing, None, 0)
        );

        let healthy = observation(MonitorState::Healthy);
        assert_eq!(
            next_location_state(MonitorState::Failing, None, 0, &healthy),
            (MonitorState::Failing, Some(MonitorState::Healthy), 1)
        );
        assert_eq!(
            next_location_state(
                MonitorState::Failing,
                Some(MonitorState::Healthy),
                1,
                &healthy,
            ),
            (MonitorState::Healthy, None, 0)
        );
    }
}
