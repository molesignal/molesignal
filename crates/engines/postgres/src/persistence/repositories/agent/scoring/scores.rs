// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 被打分的对象与它们的分数：对象一行，每个打分器一行分数；列表按对象，最新的在前。

use sqlx::{PgPool, Row, postgres::PgRow, types::Json};

use super::{json, micros, optional_text, sources, sqlx_err, text};
use crate::{
    agent::scoring::{
        Attribution, RULES_SCORER, Score, ScoreSubject, ScoreTarget, ScoredFilter, ScoredTarget,
        TargetSource,
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 对象的列与分数的列：同名的列在分数这边加前缀。
pub(super) const SUBJECT_COLUMNS: &str = "t.org_id, t.target_kind, t.target_id, t.source, \
    t.question, t.answer, t.chat_id, t.team_id, t.automation_id, t.user_id, \
    t.prompt_template_id, t.prompt_version, t.prompt_key, t.provider_id, t.model, t.skill, \
    t.skill_version, t.target_at";

const SCORE_COLUMNS: &str = "s.id AS score_id, s.scorer, s.score, s.dimensions, s.reason, \
    s.samples, s.evaluator_model, s.created_at AS score_created_at, \
    s.updated_at AS score_updated_at";

/// 每行分数排在它的对象里：规则打分在前，评审按评分的先后。
const SCORE_ORDER: &str = "(s.scorer = 'rules') DESC, s.created_at, s.scorer";

pub(super) fn subject_row(row: &PgRow) -> Result<ScoreSubject> {
    let id = |column: &str| -> Result<Option<Id>> { Ok(optional_text(row, column)?.map(Id)) };

    Ok(ScoreSubject {
        org_id: Id(text(row, "org_id")?),
        target_kind: ScoreTarget::parse(&text(row, "target_kind")?)?,
        target_id: text(row, "target_id")?,
        source: TargetSource::parse(&text(row, "source")?)?,
        question: text(row, "question")?,
        answer: text(row, "answer")?,
        attribution: Attribution {
            chat_id: id("chat_id")?,
            team_id: id("team_id")?,
            automation_id: id("automation_id")?,
            user_id: id("user_id")?,
            prompt_template_id: optional_text(row, "prompt_template_id")?,
            prompt_version: row.try_get("prompt_version").map_err(sqlx_err)?,
            prompt_key: optional_text(row, "prompt_key")?,
            provider_id: id("provider_id")?,
            model: optional_text(row, "model")?,
            skill: optional_text(row, "skill")?,
            skill_version: optional_text(row, "skill_version")?,
        },
        target_at: micros(row, "target_at")?,
    })
}

/// 联查出来的一行里的分数；对象还没有分数时为 `None`。
fn score_row(row: &PgRow, subject: &ScoreSubject) -> Result<Option<Score>> {
    let Some(id) = optional_text(row, "score_id")? else {
        return Ok(None);
    };

    Ok(Some(Score {
        id: Id(id),
        org_id: subject.org_id.clone(),
        target_kind: subject.target_kind,
        target_id: subject.target_id.clone(),
        scorer: text(row, "scorer")?,
        score: row.try_get("score").map_err(sqlx_err)?,
        dimensions: json(row, "dimensions")?,
        reason: text(row, "reason")?,
        samples: row.try_get("samples").map_err(sqlx_err)?,
        evaluator_model: optional_text(row, "evaluator_model")?,
        created_at: micros(row, "score_created_at")?,
        updated_at: micros(row, "score_updated_at")?,
    }))
}

pub(super) async fn put_subject(pool: &PgPool, subject: &ScoreSubject) -> Result<()> {
    let attribution = &subject.attribution;
    let id = |value: &Option<Id>| value.as_ref().map(|id| id.0.clone());

    sqlx::query(
        "INSERT INTO agent_score_subjects
         (org_id, target_kind, target_id, source, question, answer, chat_id, team_id,
          automation_id, user_id, prompt_template_id, prompt_version, prompt_key, provider_id,
          model, skill, skill_version, target_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18,
                 clock_timestamp())
         ON CONFLICT (org_id, target_kind, target_id) DO UPDATE SET
           source = EXCLUDED.source,
           question = EXCLUDED.question,
           answer = EXCLUDED.answer,
           chat_id = EXCLUDED.chat_id,
           team_id = EXCLUDED.team_id,
           automation_id = EXCLUDED.automation_id,
           user_id = EXCLUDED.user_id,
           prompt_template_id = EXCLUDED.prompt_template_id,
           prompt_version = EXCLUDED.prompt_version,
           prompt_key = EXCLUDED.prompt_key,
           provider_id = EXCLUDED.provider_id,
           model = EXCLUDED.model,
           skill = EXCLUDED.skill,
           skill_version = EXCLUDED.skill_version,
           target_at = EXCLUDED.target_at,
           updated_at = EXCLUDED.updated_at",
    )
    .bind(&subject.org_id.0)
    .bind(subject.target_kind.as_str())
    .bind(&subject.target_id)
    .bind(subject.source.as_str())
    .bind(&subject.question)
    .bind(&subject.answer)
    .bind(id(&attribution.chat_id))
    .bind(id(&attribution.team_id))
    .bind(id(&attribution.automation_id))
    .bind(id(&attribution.user_id))
    .bind(attribution.prompt_template_id.as_deref())
    .bind(attribution.prompt_version)
    .bind(attribution.prompt_key.as_deref())
    .bind(id(&attribution.provider_id))
    .bind(attribution.model.as_deref())
    .bind(attribution.skill.as_deref())
    .bind(attribution.skill_version.as_deref())
    .bind(subject.target_at)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn put(pool: &PgPool, score: &Score) -> Result<()> {
    sqlx::query(
        "INSERT INTO agent_scores
         (id, org_id, target_kind, target_id, scorer, score, dimensions, reason, samples,
          evaluator_model, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         ON CONFLICT (org_id, target_kind, target_id, scorer) DO UPDATE SET
           score = EXCLUDED.score,
           dimensions = EXCLUDED.dimensions,
           reason = EXCLUDED.reason,
           samples = EXCLUDED.samples,
           evaluator_model = EXCLUDED.evaluator_model,
           updated_at = EXCLUDED.updated_at",
    )
    .bind(&score.id.0)
    .bind(&score.org_id.0)
    .bind(score.target_kind.as_str())
    .bind(&score.target_id)
    .bind(&score.scorer)
    .bind(score.score.clamp(0.0, 1.0))
    .bind(Json(&score.dimensions))
    .bind(&score.reason)
    .bind(score.samples)
    .bind(score.evaluator_model.as_deref())
    .bind(score.created_at)
    .bind(score.updated_at)
    .execute(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(())
}

pub(super) async fn scored(
    pool: &PgPool,
    org_id: &Id,
    kind: ScoreTarget,
    target_id: &str,
) -> Result<Option<ScoredTarget>> {
    let rows = sqlx::query(&format!(
        "SELECT {SUBJECT_COLUMNS}, {SCORE_COLUMNS}
         FROM agent_score_subjects t
         LEFT JOIN agent_scores s
           ON s.org_id = t.org_id AND s.target_kind = t.target_kind AND s.target_id = t.target_id
         WHERE t.org_id = $1 AND t.target_kind = $2 AND t.target_id = $3
         ORDER BY {SCORE_ORDER}"
    ))
    .bind(&org_id.0)
    .bind(kind.as_str())
    .bind(target_id)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    Ok(grouped(rows)?.into_iter().next())
}

/// 打过分的对象，最新的在前；个人对话不在其中。`below` 比的是对象的分数：有评审评过时是评审
/// 的平均，否则是规则打分。
pub(super) async fn list(
    pool: &PgPool,
    org_id: &Id,
    filter: &ScoredFilter,
) -> Result<Vec<ScoredTarget>> {
    let (before_at, before_id) = match &filter.before {
        Some((at, id)) => (Some(at.0), Some(id.as_str())),
        None => (None, None),
    };

    let rows = sqlx::query(&format!(
        "WITH page AS (
           SELECT t.target_kind, t.target_id, t.target_at
           FROM agent_score_subjects t
           WHERE t.org_id = $1 AND t.target_at >= $2 AND t.source <> 'personal_chat'
             AND ($3::TEXT IS NULL OR t.source = $3)
             AND ($5::TIMESTAMPTZ IS NULL OR (t.target_at, t.target_id) < ($5, $6::TEXT))
             AND ($4::FLOAT8 IS NULL OR (
               SELECT COALESCE(
                 AVG(x.score) FILTER (WHERE x.scorer <> '{RULES_SCORER}'),
                 MAX(x.score) FILTER (WHERE x.scorer = '{RULES_SCORER}'))
               FROM agent_scores x
               WHERE x.org_id = t.org_id AND x.target_kind = t.target_kind
                 AND x.target_id = t.target_id) < $4)
           ORDER BY t.target_at DESC, t.target_id DESC
           LIMIT $7
         )
         SELECT {SUBJECT_COLUMNS}, {SCORE_COLUMNS}
         FROM page p
         JOIN agent_score_subjects t
           ON t.org_id = $1 AND t.target_kind = p.target_kind AND t.target_id = p.target_id
         LEFT JOIN agent_scores s
           ON s.org_id = t.org_id AND s.target_kind = t.target_kind AND s.target_id = t.target_id
         ORDER BY t.target_at DESC, t.target_id DESC, {SCORE_ORDER}"
    ))
    .bind(&org_id.0)
    .bind(filter.since)
    .bind(filter.source.map(TargetSource::as_str))
    .bind(filter.below)
    .bind(before_at.map(TimestampMicros))
    .bind(before_id)
    .bind(filter.limit)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    grouped(rows)
}

/// 一个评审最近评过的对象，最新的在前，带着它们的全部分数。
pub(super) async fn recent_evaluated(
    pool: &PgPool,
    org_id: &Id,
    evaluator: &Id,
    wanted: &[TargetSource],
    limit: i64,
) -> Result<Vec<ScoredTarget>> {
    let rows = sqlx::query(&format!(
        "WITH page AS (
           SELECT t.target_kind, t.target_id
           FROM agent_score_subjects t
           JOIN agent_scores x
             ON x.org_id = t.org_id AND x.target_kind = t.target_kind
            AND x.target_id = t.target_id AND x.scorer = $2
           WHERE t.org_id = $1 AND t.source = ANY($3)
           ORDER BY t.target_at DESC, t.target_id DESC
           LIMIT $4
         )
         SELECT {SUBJECT_COLUMNS}, {SCORE_COLUMNS}
         FROM page p
         JOIN agent_score_subjects t
           ON t.org_id = $1 AND t.target_kind = p.target_kind AND t.target_id = p.target_id
         LEFT JOIN agent_scores s
           ON s.org_id = t.org_id AND s.target_kind = t.target_kind AND s.target_id = t.target_id
         ORDER BY t.target_at DESC, t.target_id DESC, {SCORE_ORDER}"
    ))
    .bind(&org_id.0)
    .bind(&evaluator.0)
    .bind(sources(wanted))
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    grouped(rows)
}

/// 按对象分组：行已经按对象排好，同一个对象的行挨在一起。
fn grouped(rows: Vec<PgRow>) -> Result<Vec<ScoredTarget>> {
    let mut targets: Vec<ScoredTarget> = Vec::new();

    for row in rows {
        let kind = ScoreTarget::parse(&text(&row, "target_kind")?)?;
        let target_id = text(&row, "target_id")?;

        let same = targets.last().is_some_and(|target| {
            target.subject.target_kind == kind && target.subject.target_id == target_id
        });

        if !same {
            targets.push(ScoredTarget {
                subject: subject_row(&row)?,
                scores: Vec::new(),
            });
        }

        let target = targets.last_mut().expect("a target was just pushed");

        if let Some(score) = score_row(&row, &target.subject)? {
            target.scores.push(score);
        }
    }

    Ok(targets)
}
