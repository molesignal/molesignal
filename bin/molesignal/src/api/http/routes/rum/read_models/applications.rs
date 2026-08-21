// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde::Serialize;

use super::{ReadModelContext, ReadModelQuery};
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::permission,
    infra::rum::read_model::{RumReadModelReader, RumScope},
    shared::Result,
};

const SESSION_COLUMNS: &[&str] = &[
    "_timestamp",
    "started_at_micros",
    "session_id",
    "user_id",
    "error_count",
    "application",
    "environment",
    "version",
];
const ACTION_COLUMNS: &[&str] = &[
    "_timestamp",
    "ts_micros",
    "session_id",
    "type",
    "application",
    "environment",
    "version",
    "lcp_ms",
];

#[derive(Debug, Serialize)]
pub(super) struct ApplicationsResponse {
    items: Vec<ApplicationSummary>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplicationSummary {
    application: String,
    environments: Vec<String>,
    versions: Vec<String>,
    users: i64,
    sessions: i64,
    error_free_rate: f64,
    lcp_p75: f64,
}

#[derive(Default)]
struct SessionAccumulator {
    applications: BTreeMap<String, ApplicationAccumulator>,
    seen_sessions: HashSet<String>,
}

#[derive(Default)]
struct ApplicationAccumulator {
    environments: BTreeSet<String>,
    versions: BTreeSet<String>,
    users: HashSet<String>,
    sessions: i64,
    error_free_sessions: i64,
}

#[permission(any("streams.query", "sys.telemetry.read"))]
pub(super) async fn summary(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(request): Query<ReadModelQuery>,
) -> Result<Json<ApplicationsResponse>> {
    let context = ReadModelContext::resolve(request)?;
    let reader = RumReadModelReader::new(
        state.storage.catalog_files.clone(),
        state.storage.read_store.clone(),
    )
    .with_catalog_source(state.storage.catalog_query.clone());
    let scope = RumScope {
        environment: context.environment.as_deref(),
        version: context.version.as_deref(),
        ..RumScope::default()
    };
    let mut sessions = SessionAccumulator::default();
    let mut lcp_by_session = HashMap::<(String, String), f64>::new();
    let range = context.time_range();
    let (session_stats, action_stats) = tokio::try_join!(
        reader.visit_sessions(&iam.org_id, range, SESSION_COLUMNS, |record| {
            if !record.matches_scope(scope)
                || !sessions.seen_sessions.insert(record.session_id.clone())
            {
                return;
            }
            let application = record.application.unwrap_or_default();
            let item = sessions.applications.entry(application).or_default();
            item.sessions += 1;
            item.error_free_sessions += i64::from(record.error_count == 0);
            item.users
                .insert(record.user_id.unwrap_or(record.session_id));
            if let Some(value) = record.environment {
                item.environments.insert(value);
            }
            if let Some(value) = record.version {
                item.versions.insert(value);
            }
        }),
        reader.visit_actions(&iam.org_id, range, ACTION_COLUMNS, |record| {
            if record.event_type != "view" || !record.matches_scope(scope) {
                return;
            }
            let Some(lcp) = record.lcp_ms.filter(|value| value.is_finite()) else {
                return;
            };
            let key = (record.application.unwrap_or_default(), record.session_id);
            lcp_by_session
                .entry(key)
                .and_modify(|value| *value = value.max(lcp))
                .or_insert(lcp);
        }),
    )?;
    tracing::debug!(
        session_files = session_stats.files,
        session_rows = session_stats.rows,
        action_files = action_stats.files,
        action_rows = action_stats.rows,
        "RUM applications read model completed"
    );

    let mut lcp_by_application = BTreeMap::<String, Vec<f64>>::new();
    for ((application, _), lcp) in lcp_by_session {
        lcp_by_application.entry(application).or_default().push(lcp);
    }
    let mut items = sessions
        .applications
        .into_iter()
        .map(|(application, item)| {
            let sessions = item.sessions;
            ApplicationSummary {
                lcp_p75: lcp_by_application
                    .get_mut(&application)
                    .map(|values| percentile(values, 0.75))
                    .unwrap_or_default(),
                application,
                environments: item.environments.into_iter().collect(),
                versions: item.versions.into_iter().collect(),
                users: i64::try_from(item.users.len()).unwrap_or(i64::MAX),
                sessions,
                error_free_rate: if sessions == 0 {
                    0.0
                } else {
                    item.error_free_sessions as f64 / sessions as f64
                },
            }
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .sessions
            .cmp(&left.sessions)
            .then_with(|| left.application.cmp(&right.application))
    });
    Ok(Json(ApplicationsResponse { items }))
}

fn percentile(values: &mut [f64], quantile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() as f64 * quantile).ceil() as usize)
        .saturating_sub(1)
        .min(values.len() - 1);
    values[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_uses_nearest_rank() {
        assert_eq!(percentile(&mut [1.0, 4.0, 2.0, 3.0], 0.75), 3.0);
        assert_eq!(percentile(&mut [], 0.75), 0.0);
    }
}
