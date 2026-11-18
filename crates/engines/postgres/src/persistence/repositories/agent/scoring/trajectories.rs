// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What an answer's tool calls said about its data, kept with its scored subject
//! (`agent_score_subjects.trajectory`), and read back for the data gaps of a period.

use agent::scoring::{ScoreTarget, TargetSource, gaps::TrajectoryRow, trajectory::Trajectory};
use kernel::{Result, ids::Id, time::TimestampMicros};
use sqlx::{PgPool, types::Json};

use super::{json, micros, optional_text, sqlx_err, text};

pub(super) async fn put(
    pool: &PgPool,
    org_id: &Id,
    kind: ScoreTarget,
    target_id: &str,
    trajectory: Option<&Trajectory>,
) -> Result<()> {
    sqlx::query(
        "UPDATE agent_score_subjects SET trajectory = $4
         WHERE org_id = $1 AND target_kind = $2 AND target_id = $3",
    )
    .bind(&org_id.0)
    .bind(kind.as_str())
    .bind(target_id)
    .bind(trajectory.map(Json))
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn list(
    pool: &PgPool,
    org_id: &Id,
    since: TimestampMicros,
    limit: i64,
) -> Result<Vec<TrajectoryRow>> {
    let rows = sqlx::query(
        "SELECT source, target_id, chat_id, question, target_at, trajectory
         FROM agent_score_subjects
         WHERE org_id = $1 AND target_kind = 'chat_answer' AND target_at >= $2
           AND trajectory IS NOT NULL
         ORDER BY target_at DESC, target_id DESC
         LIMIT $3",
    )
    .bind(&org_id.0)
    .bind(since)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    rows.iter()
        .map(|row| {
            Ok(TrajectoryRow {
                source: TargetSource::parse(&text(row, "source")?)?,
                target_id: text(row, "target_id")?,
                chat_id: optional_text(row, "chat_id")?.map(Id),
                question: text(row, "question")?,
                target_at: micros(row, "target_at")?,
                trajectory: json(row, "trajectory")?,
            })
        })
        .collect()
}
