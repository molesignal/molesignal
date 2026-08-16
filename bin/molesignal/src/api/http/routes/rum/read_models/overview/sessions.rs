// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::{BTreeMap, HashSet};

use super::{super::ReadModelContext, model::SessionAggregate};
use crate::{
    api::AppState,
    app::iam::IamContext,
    infra::rum::read_model::{RumReadModelReader, RumScope},
    shared::Result,
};

const COLUMNS: &[&str] = &[
    "_timestamp",
    "started_at_micros",
    "session_id",
    "user_id",
    "error_count",
    "application",
    "environment",
    "version",
    "country",
    "browser",
    "device",
];

pub(super) async fn load(
    state: &AppState,
    iam: &IamContext,
    context: &ReadModelContext,
) -> Result<SessionAggregate> {
    let reader = RumReadModelReader::new(
        state.storage.parquet_file_meta.clone(),
        state.storage.object_store.clone(),
    );
    let scope = RumScope {
        application: context.application.as_deref(),
        environment: context.environment.as_deref(),
        version: context.version.as_deref(),
        country: context.country.as_deref(),
        device: context.device.as_deref(),
    };
    let mut seen_sessions = HashSet::new();
    let mut users = HashSet::new();
    let mut error_free_sessions = 0_i64;
    let mut browser_devices = BTreeMap::<String, i64>::new();
    let mut regions = BTreeMap::<String, i64>::new();
    let mut aggregate = SessionAggregate::default();
    let stats = reader
        .visit_sessions(&iam.org_id, context.time_range(), COLUMNS, |record| {
            if !record.matches_scope(scope) || !seen_sessions.insert(record.session_id.clone()) {
                return;
            }
            users.insert(record.user_id.clone().unwrap_or(record.session_id.clone()));
            error_free_sessions += i64::from(record.error_count == 0);
            if !context.summary_only {
                let browser = record.browser.as_deref().unwrap_or_default();
                let device = record.device.as_deref().unwrap_or_default();
                let browser_device = match (browser.is_empty(), device.is_empty()) {
                    (false, false) => format!("{browser} · {device}"),
                    (false, true) => browser.to_string(),
                    (true, false) => device.to_string(),
                    (true, true) => "—".into(),
                };
                *browser_devices.entry(browser_device).or_default() += 1;
                *regions
                    .entry(record.country.clone().unwrap_or_else(|| "—".into()))
                    .or_default() += 1;
                push_facet(
                    &mut aggregate,
                    "facet_application",
                    record.application.as_deref(),
                );
                push_facet(
                    &mut aggregate,
                    "facet_environment",
                    record.environment.as_deref(),
                );
                push_facet(&mut aggregate, "facet_version", record.version.as_deref());
                push_facet(&mut aggregate, "facet_country", record.country.as_deref());
                push_facet(&mut aggregate, "facet_device", record.device.as_deref());
            }
        })
        .await?;
    aggregate.users = i64::try_from(users.len()).unwrap_or(i64::MAX);
    aggregate.sessions = i64::try_from(seen_sessions.len()).unwrap_or(i64::MAX);
    aggregate.error_free_rate = if aggregate.sessions == 0 {
        0.0
    } else {
        error_free_sessions as f64 / aggregate.sessions as f64
    };
    aggregate.browser_devices = browser_devices.into_iter().collect();
    aggregate.regions = regions.into_iter().collect();
    tracing::debug!(
        scanned_files = stats.files,
        scanned_rows = stats.rows,
        sessions = aggregate.sessions,
        "RUM overview session read model completed"
    );
    Ok(aggregate)
}

fn push_facet(aggregate: &mut SessionAggregate, kind: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        aggregate.facets.push(kind, value.to_string());
    }
}
