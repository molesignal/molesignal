// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! RUM page-specific aggregate read models.

use axum::{Router, routing::get};
use serde::Deserialize;

use super::list::normalize_text;
use crate::{
    api::AppState,
    shared::{
        Error, Result,
        time::{TimeRange, TimestampMicros},
    },
};

mod applications;
mod detail;
mod overview;
mod performance;

const DEFAULT_WINDOW_MICROS: i64 = 24 * 60 * 60 * 1_000_000;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/rum/applications/summary", get(applications::summary))
        .route("/rum/sessions/{id}", get(detail::session))
        .route("/rum/errors/{fingerprint}", get(detail::error))
        .route("/rum/overview", get(overview::read))
        .route("/rum/overview/insights", get(overview::insights))
        .route("/rum/performance/vitals", get(performance::vitals))
        .route("/rum/performance/apis", get(performance::apis))
        .route("/rum/performance/errors", get(performance::errors))
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ReadModelQuery {
    pub(super) from: Option<i64>,
    pub(super) to: Option<i64>,
    pub(super) application: Option<String>,
    pub(super) environment: Option<String>,
    pub(super) version: Option<String>,
    pub(super) country: Option<String>,
    pub(super) device: Option<String>,
    #[serde(default)]
    pub(super) summary_only: bool,
}

#[derive(Clone, Debug)]
pub(super) struct ReadModelContext {
    pub(super) from: i64,
    pub(super) to: i64,
    pub(super) application: Option<String>,
    pub(super) environment: Option<String>,
    pub(super) version: Option<String>,
    pub(super) country: Option<String>,
    pub(super) device: Option<String>,
    pub(super) summary_only: bool,
}

impl ReadModelContext {
    pub(super) fn resolve(request: ReadModelQuery) -> Result<Self> {
        let now = TimestampMicros::now().0;
        let from = request
            .from
            .unwrap_or_else(|| now.saturating_sub(DEFAULT_WINDOW_MICROS));
        let to = request.to.unwrap_or(now);
        if to <= from {
            return Err(Error::invalid("RUM range end must be greater than start"));
        }
        Ok(Self {
            from,
            to,
            application: normalize_text(request.application, 128),
            environment: normalize_text(request.environment, 128),
            version: normalize_text(request.version, 128),
            country: normalize_text(request.country, 128),
            device: normalize_text(request.device, 128),
            summary_only: request.summary_only,
        })
    }

    pub(super) fn time_range(&self) -> TimeRange {
        TimeRange::new(TimestampMicros(self.from), TimestampMicros(self.to))
    }
}
