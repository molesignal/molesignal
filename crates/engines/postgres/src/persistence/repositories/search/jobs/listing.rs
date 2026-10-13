// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `search_jobs` 的列表读取：按状态和提交人筛选、翻页，以及各状态的任务数。

use serde::Serialize;
use sqlx::{PgPool, Row};

use super::{COLS, SearchJob, SearchJobState, row_to};
use crate::{
    persistence::sqlx_err,
    shared::{Result, ids::Id},
};

/// 列表要哪一段任务。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchJobListQuery {
    /// 只要这些状态的任务；为空表示不按状态筛选。
    pub states: Vec<SearchJobState>,
    /// 只要这位用户提交的任务；`None` 表示组织里所有人的。
    pub submitter: Option<Id>,
    pub limit: i64,
    /// 跳过最近提交的多少条。
    pub offset: i64,
}

/// 一个组织各状态的任务数。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SearchJobCounts {
    pub pending: i64,
    pub running: i64,
    pub done: i64,
    pub failed: i64,
    pub cancelled: i64,
}

impl SearchJobCounts {
    pub fn total(&self) -> i64 {
        self.pending + self.running + self.done + self.failed + self.cancelled
    }

    fn add(&mut self, state: SearchJobState, jobs: i64) {
        let slot = match state {
            SearchJobState::Pending => &mut self.pending,
            SearchJobState::Running => &mut self.running,
            SearchJobState::Done => &mut self.done,
            SearchJobState::Failed => &mut self.failed,
            SearchJobState::Cancelled => &mut self.cancelled,
        };

        *slot += jobs;
    }
}

pub(super) async fn list(
    pool: &PgPool,
    org_id: &Id,
    query: &SearchJobListQuery,
) -> Result<Vec<SearchJob>> {
    let states: Vec<&str> = query.states.iter().map(SearchJobState::as_str).collect();
    let submitter = query.submitter.as_ref().map(Id::as_str);

    // 状态数组为空、提交人为 NULL 时各自不筛选。`id` 给同一微秒提交的任务一个确定的先后，
    // 翻页时不会重复或漏掉。
    let sql = format!(
        "SELECT {COLS} FROM search_jobs
          WHERE org_id = $1
            AND (cardinality($2::text[]) = 0 OR state = ANY($2::text[]))
            AND ($3::text IS NULL OR user_id = $3::text)
          ORDER BY submitted_at_micros DESC, id DESC
          LIMIT $4 OFFSET $5"
    );

    let rows = sqlx::query(&sql)
        .bind(&org_id.0)
        .bind(&states)
        .bind(submitter)
        .bind(query.limit)
        .bind(query.offset)
        .fetch_all(pool)
        .await
        .map_err(sqlx_err)?;

    Ok(rows.into_iter().map(row_to).collect())
}

pub(super) async fn count_by_state(
    pool: &PgPool,
    org_id: &Id,
    submitter: Option<&Id>,
) -> Result<SearchJobCounts> {
    let rows = sqlx::query(
        "SELECT state, COUNT(*) AS jobs FROM search_jobs
          WHERE org_id = $1
            AND ($2::text IS NULL OR user_id = $2::text)
          GROUP BY state",
    )
    .bind(&org_id.0)
    .bind(submitter.map(Id::as_str))
    .fetch_all(pool)
    .await
    .map_err(sqlx_err)?;

    let mut counts = SearchJobCounts::default();

    for row in rows {
        let state: String = row.try_get("state").map_err(sqlx_err)?;
        let jobs: i64 = row.try_get("jobs").map_err(sqlx_err)?;

        // 表里只会有五个状态；认不出的值不计入任何一栏，而不是算作排队。
        if let Some(state) = SearchJobState::try_parse(&state) {
            counts.add(state, jobs);
        }
    }

    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_add_up_by_state() {
        let mut counts = SearchJobCounts::default();
        assert_eq!(counts.total(), 0);

        counts.add(SearchJobState::Pending, 2);
        counts.add(SearchJobState::Running, 1);
        counts.add(SearchJobState::Done, 40);
        counts.add(SearchJobState::Failed, 3);
        counts.add(SearchJobState::Cancelled, 4);

        assert_eq!(
            counts,
            SearchJobCounts {
                pending: 2,
                running: 1,
                done: 40,
                failed: 3,
                cancelled: 4,
            }
        );
        assert_eq!(counts.total(), 50);
    }
}
