// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::Serialize;

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct OverviewSummaryResponse {
    metrics: OverviewMetrics,
    browser_devices: Vec<DimensionShare>,
    regions: Vec<DimensionShare>,
    facets: OverviewFacets,
}

impl OverviewSummaryResponse {
    pub(super) fn from_parts(sessions: SessionAggregate, actions: ActionAggregate) -> Self {
        Self {
            metrics: OverviewMetrics {
                users: sessions.users,
                sessions: sessions.sessions,
                error_free_rate: sessions.error_free_rate,
                lcp_p75: actions.lcp_p75,
                inp_p75: actions.inp_p75,
                cls_p75: actions.cls_p75,
            },
            browser_devices: shares(sessions.browser_devices, sessions.sessions),
            regions: shares(sessions.regions, sessions.sessions),
            facets: sessions.facets,
        }
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in super::super) struct OverviewInsightsResponse {
    trend: Vec<ExperienceBucket>,
    satisfaction: SatisfactionCounts,
    slow_pages: Vec<SlowPage>,
    frequent_errors: Vec<FrequentError>,
}

impl OverviewInsightsResponse {
    pub(super) fn from_parts(
        actions: ActionAggregate,
        frequent_errors: Vec<FrequentError>,
    ) -> Self {
        Self {
            trend: actions.trend,
            satisfaction: actions.satisfaction,
            slow_pages: actions.slow_pages,
            frequent_errors,
        }
    }
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OverviewMetrics {
    users: i64,
    sessions: i64,
    error_free_rate: f64,
    lcp_p75: f64,
    inp_p75: f64,
    cls_p75: f64,
}

#[derive(Debug, Default)]
pub(super) struct SessionAggregate {
    pub(super) users: i64,
    pub(super) sessions: i64,
    pub(super) error_free_rate: f64,
    pub(super) browser_devices: Vec<(String, i64)>,
    pub(super) regions: Vec<(String, i64)>,
    pub(super) facets: OverviewFacets,
}

#[derive(Debug, Default)]
pub(super) struct ActionAggregate {
    pub(super) lcp_p75: f64,
    pub(super) inp_p75: f64,
    pub(super) cls_p75: f64,
    pub(super) trend: Vec<ExperienceBucket>,
    pub(super) satisfaction: SatisfactionCounts,
    pub(super) slow_pages: Vec<SlowPage>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExperienceBucket {
    pub(super) start: i64,
    pub(super) good: i64,
    pub(super) needs: i64,
    pub(super) poor: i64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SatisfactionCounts {
    pub(super) good: i64,
    pub(super) needs_improvement: i64,
    pub(super) poor: i64,
    pub(super) total: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SlowPage {
    pub(super) page: String,
    pub(super) p75: f64,
    pub(super) sessions: i64,
    pub(super) error_rate: f64,
    pub(super) grade: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FrequentError {
    pub(super) fingerprint: String,
    pub(super) message: String,
    pub(super) users: i64,
    pub(super) version: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct DimensionShare {
    label: String,
    count: i64,
    share: f64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct OverviewFacets {
    pub(super) applications: Vec<String>,
    pub(super) environments: Vec<String>,
    pub(super) versions: Vec<String>,
    pub(super) countries: Vec<String>,
    pub(super) devices: Vec<String>,
}

impl OverviewFacets {
    pub(super) fn push(&mut self, kind: &str, value: String) {
        let target = match kind {
            "facet_application" => &mut self.applications,
            "facet_environment" => &mut self.environments,
            "facet_version" => &mut self.versions,
            "facet_country" => &mut self.countries,
            "facet_device" => &mut self.devices,
            _ => return,
        };
        if !target.contains(&value) {
            target.push(value);
            target.sort();
        }
    }
}

fn shares(mut values: Vec<(String, i64)>, total: i64) -> Vec<DimensionShare> {
    values.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    values
        .into_iter()
        .take(6)
        .map(|(label, count)| DimensionShare {
            label,
            count,
            share: if total == 0 {
                0.0
            } else {
                count as f64 / total as f64
            },
        })
        .collect()
}

pub(super) fn lcp_grade(value: f64) -> &'static str {
    if value > 4_000.0 {
        "poor"
    } else if value > 2_500.0 {
        "needs_improvement"
    } else if value > 0.0 {
        "good"
    } else {
        "unknown"
    }
}
