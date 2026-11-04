// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一段时间里的质量：合计、每天、规则打分的各个维度，以及按来源、Prompt 版本、Skill 与模型
//! 分组。一个对象的规则打分一行，评审每个一行：对象数按规则打分的行数，评审评过的按对象去重。

use sqlx::{PgPool, Row, postgres::PgRow};

use super::{optional_text, sqlx_err, text};
use crate::{
    agent::scoring::{
        DailyQuality, DimensionMean, GroupQuality, OverviewFilter, QualityOverview, RULES_SCORER,
        RuleDimension,
    },
    shared::{Result, ids::Id},
};

/// 每种分组最多列这么多个。
const GROUP_LIMIT: i64 = 10;

/// 分数连同它的对象。
const FROM: &str = "agent_scores s JOIN agent_score_subjects t \
    ON t.org_id = s.org_id AND t.target_kind = s.target_kind AND t.target_id = s.target_id";

/// 每条 SQL 都限定组织（$1）、对象产生的时间（$2 起）与来源（$3，可空）。
const WHERE: &str = "s.org_id = $1 AND t.target_at >= $2 AND ($3::TEXT IS NULL OR t.source = $3)";

/// 对象数、规则打分的平均、评审评过的对象数与评审的平均。
fn measures() -> String {
    format!(
        "COUNT(*) FILTER (WHERE s.scorer = '{RULES_SCORER}') AS answers,
         AVG(s.score) FILTER (WHERE s.scorer = '{RULES_SCORER}') AS rules_mean,
         COUNT(DISTINCT s.target_kind || ':' || s.target_id)
           FILTER (WHERE s.scorer <> '{RULES_SCORER}') AS evaluated,
         AVG(s.score) FILTER (WHERE s.scorer <> '{RULES_SCORER}') AS evaluator_mean"
    )
}

pub(super) async fn overview(
    pool: &PgPool,
    org_id: &Id,
    filter: &OverviewFilter,
) -> Result<QualityOverview> {
    let source = filter.source.map(|source| source.as_str());

    let totals = sqlx::query(&format!(
        "SELECT {measures},
           COUNT(*) FILTER (WHERE s.scorer = '{RULES_SCORER}'
             AND s.dimensions @> '[{{\"dimension\": \"feedback\", \"detail\": {{\"rating\": \"up\"}}}}]')
             AS thumbs_up,
           COUNT(*) FILTER (WHERE s.scorer = '{RULES_SCORER}'
             AND s.dimensions @> '[{{\"dimension\": \"feedback\", \"detail\": {{\"rating\": \"down\"}}}}]')
             AS thumbs_down
         FROM {FROM} WHERE {WHERE}",
        measures = measures(),
    ))
    .bind(&org_id.0)
    .bind(filter.since)
    .bind(source)
    .fetch_one(pool)
    .await
    .map_err(sqlx_err)?;

    let pending_reviews: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM agent_score_reviews WHERE org_id = $1 AND status = 'pending'",
    )
    .bind(&org_id.0)
    .fetch_one(pool)
    .await
    .map_err(sqlx_err)?;

    let daily = sqlx::query(&format!(
        "SELECT floor(EXTRACT(EPOCH FROM t.target_at) / 86400)::BIGINT AS day, {measures}
         FROM {FROM} WHERE {WHERE}
         GROUP BY 1 ORDER BY 1",
        measures = measures(),
    ))
    .bind(&org_id.0)
    .bind(filter.since)
    .bind(source)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .into_iter()
    .map(|row| {
        Ok(DailyQuality {
            day: row.try_get("day").map_err(sqlx_err)?,
            answers: row.try_get("answers").map_err(sqlx_err)?,
            rules_mean: row.try_get("rules_mean").map_err(sqlx_err)?,
            evaluated: row.try_get("evaluated").map_err(sqlx_err)?,
            evaluator_mean: row.try_get("evaluator_mean").map_err(sqlx_err)?,
        })
    })
    .collect::<Result<Vec<_>>>()?;

    let dimensions = sqlx::query(&format!(
        "SELECT d->>'dimension' AS dimension, AVG((d->>'score')::FLOAT8) AS mean,
                COUNT(*) AS answers
         FROM {FROM} CROSS JOIN LATERAL jsonb_array_elements(s.dimensions) d
         WHERE s.scorer = '{RULES_SCORER}' AND {WHERE}
         GROUP BY 1"
    ))
    .bind(&org_id.0)
    .bind(filter.since)
    .bind(source)
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .into_iter()
    .filter_map(|row| {
        let dimension = RuleDimension::parse(&text(&row, "dimension").ok()?)?;

        Some(Ok(DimensionMean {
            dimension,
            mean: row.try_get("mean").ok()?,
            answers: row.try_get("answers").ok()?,
        }))
    })
    .collect::<Result<Vec<_>>>()?;

    Ok(QualityOverview {
        answers: totals.try_get("answers").map_err(sqlx_err)?,
        rules_mean: totals.try_get("rules_mean").map_err(sqlx_err)?,
        evaluated: totals.try_get("evaluated").map_err(sqlx_err)?,
        evaluator_mean: totals.try_get("evaluator_mean").map_err(sqlx_err)?,
        thumbs_up: totals.try_get("thumbs_up").map_err(sqlx_err)?,
        thumbs_down: totals.try_get("thumbs_down").map_err(sqlx_err)?,
        pending_reviews,
        daily,
        dimensions,
        sources: groups(pool, org_id, filter, "t.source", "NULL::TEXT").await?,
        prompts: groups(
            pool,
            org_id,
            filter,
            "COALESCE(t.prompt_template_id, t.prompt_key)",
            "t.prompt_version::TEXT",
        )
        .await?,
        skills: groups(pool, org_id, filter, "t.skill", "t.skill_version").await?,
        models: groups(pool, org_id, filter, "t.model", "NULL::TEXT").await?,
    })
}

/// 按 `key` 与 `version` 两个表达式分组，对象多的在前。
async fn groups(
    pool: &PgPool,
    org_id: &Id,
    filter: &OverviewFilter,
    key: &str,
    version: &str,
) -> Result<Vec<GroupQuality>> {
    sqlx::query(&format!(
        "SELECT {key} AS key, {version} AS version, {measures}
         FROM {FROM} WHERE {WHERE} AND {key} IS NOT NULL
         GROUP BY 1, 2
         ORDER BY answers DESC, evaluated DESC, 1
         LIMIT {GROUP_LIMIT}",
        measures = measures(),
    ))
    .bind(&org_id.0)
    .bind(filter.since)
    .bind(filter.source.map(|source| source.as_str()))
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?
    .into_iter()
    .map(group_row)
    .collect()
}

fn group_row(row: PgRow) -> Result<GroupQuality> {
    Ok(GroupQuality {
        key: text(&row, "key")?,
        version: optional_text(&row, "version")?,
        answers: row.try_get("answers").map_err(sqlx_err)?,
        rules_mean: row.try_get("rules_mean").map_err(sqlx_err)?,
        evaluated: row.try_get("evaluated").map_err(sqlx_err)?,
        evaluator_mean: row.try_get("evaluator_mean").map_err(sqlx_err)?,
    })
}
