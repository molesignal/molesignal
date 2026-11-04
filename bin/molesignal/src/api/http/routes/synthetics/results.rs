// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        iam::permission,
        synthetics::{
            ProbeOutcome, SyntheticResult, SyntheticResultListQuery, SyntheticResultPage,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/synthetics/results", get(list))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResultListQuery {
    query: Option<String>,
    outcome: Option<String>,
    location_id: Option<String>,
    /// Runs that finished at or after this, in microseconds.
    from_time: Option<i64>,
    /// Runs that finished before this.
    to_time: Option<i64>,
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_page_size")]
    per_page: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultListResponse {
    items: Vec<SyntheticResult>,
    total: u64,
    page: u32,
    per_page: u32,
}

const fn default_page() -> u32 {
    1
}

const fn default_page_size() -> u32 {
    20
}

#[permission("synthetics.results.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Query(query): Query<ResultListQuery>,
) -> Result<Json<ResultListResponse>> {
    let (filters, page, per_page) = query.normalize()?;

    let SyntheticResultPage { items, total } = state
        .synthetics
        .list_results_page(&context.org_id, &filters)
        .await?;

    Ok(Json(ResultListResponse {
        items,
        total,
        page,
        per_page,
    }))
}

impl ResultListQuery {
    fn normalize(self) -> Result<(SyntheticResultListQuery, u32, u32)> {
        let check_query = self
            .query
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());

        if check_query.as_ref().is_some_and(|value| value.len() > 200) {
            return Err(Error::invalid("synthetic result query exceeds 200 bytes"));
        }

        let outcome = self
            .outcome
            .as_deref()
            .map(|value| {
                ProbeOutcome::parse(value)
                    .ok_or_else(|| Error::invalid("invalid synthetic result outcome"))
            })
            .transpose()?;

        if let (Some(from), Some(to)) = (self.from_time, self.to_time)
            && from >= to
        {
            return Err(Error::invalid(
                "synthetic result range must end after it starts",
            ));
        }

        let page = self.page.max(1);
        let per_page = self.per_page.clamp(1, 100);

        let location_id = self
            .location_id
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .map(Id);

        Ok((
            SyntheticResultListQuery {
                check_query,
                outcome,
                location_id,
                from: self.from_time.map(TimestampMicros),
                to: self.to_time.map(TimestampMicros),
                offset: u64::from(page - 1).saturating_mul(u64::from(per_page)),
                limit: per_page,
            },
            page,
            per_page,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::ResultListQuery;

    #[test]
    fn normalizes_result_filters_and_pagination() {
        let (query, page, per_page) = ResultListQuery {
            query: Some("  checkout  ".to_owned()),
            outcome: Some("failing".to_owned()),
            location_id: Some("sin".to_owned()),
            from_time: Some(1_000),
            to_time: Some(2_000),
            page: 3,
            per_page: 50,
        }
        .normalize()
        .expect("valid query");

        assert_eq!(query.check_query.as_deref(), Some("checkout"));
        assert_eq!(
            query.outcome.map(|outcome| outcome.as_str()),
            Some("failing")
        );
        assert_eq!(
            query.location_id.as_ref().map(|id| id.as_str()),
            Some("sin")
        );
        assert_eq!(query.from.map(|from| from.0), Some(1_000));
        assert_eq!(query.to.map(|to| to.0), Some(2_000));
        assert_eq!(query.offset, 100);
        assert_eq!(query.limit, 50);
        assert_eq!((page, per_page), (3, 50));
    }

    #[test]
    fn rejects_unknown_result_outcome() {
        let error = ResultListQuery {
            query: None,
            outcome: Some("broken".to_owned()),
            location_id: None,
            from_time: None,
            to_time: None,
            page: 1,
            per_page: 20,
        }
        .normalize()
        .expect_err("unknown outcome must fail");

        assert!(
            error
                .to_string()
                .contains("invalid synthetic result outcome")
        );
    }
}
