// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What an alert rule reads from RUM: an application, narrowed to an environment, a page or a
//! version, and the figure it watches, in the unit people set thresholds in.
//!
//! The figures are the ones the RUM pages show: the overview's sessions and crashes, the
//! performance page's vitals, the errors page's issues. A rule names one of them, so the rule, its
//! preview and the page it was made from agree on what a number means. A rule made from the
//! explorer watches the number its query asks for ([`RumAlertSearch`]).

mod search;

use serde::{Deserialize, Serialize};

pub use self::search::{RumAlertFilter, RumAlertMeasure, RumAlertSearch};
use super::validate_application_id;
use crate::shared::{Error, Result};

/// The longest environment, page or version name a rule names.
const MAX_NAME_BYTES: usize = 255;

/// The shortest and the longest window a RUM rule is evaluated over, in seconds. A window longer
/// than this would read hours of rows on every evaluation.
pub const MIN_PERIOD_SECS: u32 = 60;
pub const MAX_PERIOD_SECS: u32 = 6 * 60 * 60;

/// The most samples a rule may ask for.
const MAX_MIN_SAMPLES: u32 = 1_000_000;

/// A figure of an application over the rule's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RumAlertMetric {
    /// Sessions that began in the window.
    SessionCount,
    /// Page views, or screens, that began in the window.
    ViewCount,
    /// Errors that happened in the window.
    ErrorCount,
    /// Views that met an error over all views, in percent (0–100).
    ErrorRate,
    /// Sessions without a crash over all sessions, in percent (0–100).
    CrashFreeSessions,
    /// Sessions with a frustration signal over all sessions, in percent (0–100).
    FrustrationRate,
    /// Requests of `fetch` and `xhr` that got no answer or a 5xx, over all of them, in percent.
    FailedRequestRate,
    /// The 75th percentile of the largest contentful paint, in milliseconds.
    LcpP75,
    /// The 75th percentile of the interaction to next paint, in milliseconds.
    InpP75,
    /// The 75th percentile of the cumulative layout shift, a score.
    ClsP75,
    /// The 75th percentile of the time to first byte, in milliseconds.
    TtfbP75,
    /// Issues first seen in the window.
    NewIssues,
    /// Issues that came back in the window after being resolved.
    RegressedIssues,
    /// A timing the application marks on its views (`addTiming`), by the name in `custom_name`, in
    /// milliseconds since the view began.
    CustomTiming,
    /// A duration the application measures itself (`addDurationVital`), by the name in
    /// `custom_name`, in milliseconds.
    CustomVital,
    /// How many sessions the issue that touched the most of them touched in the window: a high
    /// impact, whatever the issue and however old it is.
    IssueImpact,
    /// The number an explorer query asks for over the rows it matches, by the query in `search`.
    Query,
}

/// What a rule takes of a figure the application measures itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RumAlertStatistic {
    P50,
    #[default]
    P75,
    P90,
    P95,
    P99,
    Avg,
    /// How many times it was measured.
    Count,
}

impl RumAlertStatistic {
    /// The measure of the aggregate that computes it.
    pub fn function(self) -> &'static str {
        match self {
            Self::P50 => "p50",
            Self::P75 => "p75",
            Self::P90 => "p90",
            Self::P95 => "p95",
            Self::P99 => "p99",
            Self::Avg => "avg",
            Self::Count => "count",
        }
    }
}

impl RumAlertMetric {
    pub const ALL: [Self; 17] = [
        Self::SessionCount,
        Self::ViewCount,
        Self::ErrorCount,
        Self::ErrorRate,
        Self::CrashFreeSessions,
        Self::FrustrationRate,
        Self::FailedRequestRate,
        Self::LcpP75,
        Self::InpP75,
        Self::ClsP75,
        Self::TtfbP75,
        Self::NewIssues,
        Self::RegressedIssues,
        Self::CustomTiming,
        Self::CustomVital,
        Self::IssueImpact,
        Self::Query,
    ];

    /// The unit a value of the figure is in, as rule pages say it; a query's is its measure's
    /// ([`RumAlertQuery::unit`]).
    pub fn unit(self) -> &'static str {
        match self {
            Self::SessionCount
            | Self::ViewCount
            | Self::ErrorCount
            | Self::NewIssues
            | Self::RegressedIssues
            | Self::IssueImpact => "count",
            Self::ErrorRate
            | Self::CrashFreeSessions
            | Self::FrustrationRate
            | Self::FailedRequestRate => "percent",
            Self::LcpP75
            | Self::InpP75
            | Self::TtfbP75
            | Self::CustomTiming
            | Self::CustomVital => "milliseconds",
            Self::ClsP75 => "score",
            Self::Query => "none",
        }
    }

    /// A count is zero when nothing happened, so a rule can watch traffic stop. Every other
    /// figure has no value without the rows it is made of.
    pub fn counts_zero(self) -> bool {
        self.unit() == "count"
    }

    /// Whether the figure is about the issues the errors are grouped into, which are kept apart
    /// from the events: they have an environment but no page and no version.
    pub fn is_about_issues(self) -> bool {
        matches!(self, Self::NewIssues | Self::RegressedIssues)
    }

    /// Whether the figure is one the application measures itself, which a rule names.
    pub fn is_custom(self) -> bool {
        matches!(self, Self::CustomTiming | Self::CustomVital)
    }

    /// Whether a rule may narrow the figure to one page or screen. Sessions are not on one page,
    /// and an issue turns up on many. A query names the page among its filters.
    pub fn supports_page(self) -> bool {
        !matches!(
            self,
            Self::SessionCount
                | Self::CrashFreeSessions
                | Self::FrustrationRate
                | Self::NewIssues
                | Self::RegressedIssues
                | Self::Query
        )
    }

    /// Whether a rule may narrow the figure to one version of the application; a query names it
    /// among its filters.
    pub fn supports_version(self) -> bool {
        !self.is_about_issues() && self != Self::Query
    }

    /// Whether the figure can be watched against the same time on the days before: a count of new
    /// things has no usual level.
    pub fn supports_anomaly(self) -> bool {
        !self.is_about_issues()
    }

    /// How many of the rows a figure is made of it needs before it has a value. A percentile or a
    /// share of a handful of rows says nothing, and a rule on it would fire on every quiet night.
    pub fn default_min_samples(self) -> u32 {
        match self {
            Self::ErrorRate | Self::CrashFreeSessions | Self::FrustrationRate => 100,
            Self::FailedRequestRate => 200,
            Self::LcpP75 | Self::InpP75 | Self::ClsP75 | Self::TtfbP75 => 50,
            Self::CustomTiming | Self::CustomVital | Self::Query => 30,
            Self::SessionCount
            | Self::ViewCount
            | Self::ErrorCount
            | Self::NewIssues
            | Self::RegressedIssues
            | Self::IssueImpact => 0,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionCount => "session_count",
            Self::ViewCount => "view_count",
            Self::ErrorCount => "error_count",
            Self::ErrorRate => "error_rate",
            Self::CrashFreeSessions => "crash_free_sessions",
            Self::FrustrationRate => "frustration_rate",
            Self::FailedRequestRate => "failed_request_rate",
            Self::LcpP75 => "lcp_p75",
            Self::InpP75 => "inp_p75",
            Self::ClsP75 => "cls_p75",
            Self::TtfbP75 => "ttfb_p75",
            Self::NewIssues => "new_issues",
            Self::RegressedIssues => "regressed_issues",
            Self::CustomTiming => "custom_timing",
            Self::CustomVital => "custom_vital",
            Self::IssueImpact => "issue_impact",
            Self::Query => "query",
        }
    }
}

/// The RUM figure an alert rule watches, in place of a query statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RumAlertQuery {
    pub application_id: String,
    /// Every environment of the application when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<String>,
    /// One page (the path with its changing parts grouped) or, for an app, one screen; every page
    /// when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// One version of the application; every version when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub metric: RumAlertMetric,
    /// How many of the rows the figure is made of it needs before it has a value; the metric's
    /// own default when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_samples: Option<u32>,
    /// The name of the timing or the vital a rule on a figure the application measures watches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_name: Option<String>,
    /// What it takes of that figure: the 75th percentile when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statistic: Option<RumAlertStatistic>,
    /// The explorer query a rule on `query` watches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search: Option<RumAlertSearch>,
}

impl RumAlertQuery {
    /// Checks the query and the window it is evaluated over.
    pub fn validate(&self, period_secs: u32) -> Result<()> {
        validate_application_id(&self.application_id)?;

        let named = |value: &str, what: &str| -> Result<()> {
            if value.trim().is_empty() || value.len() > MAX_NAME_BYTES || value.contains('\0') {
                return Err(Error::invalid(format!("RUM alert {what} is invalid")));
            }

            Ok(())
        };

        if let Some(environment) = &self.environment {
            named(environment, "environment")?;
        }

        if let Some(page) = &self.page {
            named(page, "page")?;

            if !self.metric.supports_page() {
                return Err(Error::invalid(format!(
                    "`{}` cannot be narrowed to a page",
                    self.metric.as_str()
                )));
            }
        }

        if let Some(version) = &self.version {
            named(version, "version")?;

            if !self.metric.supports_version() {
                return Err(Error::invalid(format!(
                    "`{}` cannot be narrowed to a version",
                    self.metric.as_str()
                )));
            }
        }

        match (&self.custom_name, self.metric.is_custom()) {
            (Some(name), true) => named(name, "custom name")?,
            (None, true) => {
                return Err(Error::invalid(format!(
                    "`{}` needs a custom_name",
                    self.metric.as_str()
                )));
            }
            (Some(_), false) => {
                return Err(Error::invalid(format!(
                    "`{}` is not a figure with a custom_name",
                    self.metric.as_str()
                )));
            }
            (None, false) => {}
        }

        if self.statistic.is_some() && !self.metric.is_custom() {
            return Err(Error::invalid(format!(
                "`{}` takes no statistic",
                self.metric.as_str()
            )));
        }

        match (&self.search, self.metric == RumAlertMetric::Query) {
            (Some(search), true) => search.validate()?,
            (None, true) => return Err(Error::invalid("`query` needs a search")),
            (Some(_), false) => {
                return Err(Error::invalid(format!(
                    "`{}` is not a figure of a search",
                    self.metric.as_str()
                )));
            }
            (None, false) => {}
        }

        if let Some(samples) = self.min_samples {
            if self.statistic == Some(RumAlertStatistic::Count) {
                return Err(Error::invalid(
                    "a count of a custom figure needs no minimum of samples",
                ));
            }

            if self.search.as_ref().is_some_and(RumAlertSearch::adds_up) {
                return Err(Error::invalid(
                    "a count or a sum of a search needs no minimum of samples",
                ));
            }

            if self.metric.default_min_samples() == 0 {
                return Err(Error::invalid(format!(
                    "`{}` is a count and needs no minimum of samples",
                    self.metric.as_str()
                )));
            }

            if samples > MAX_MIN_SAMPLES {
                return Err(Error::invalid(format!(
                    "min_samples must be at most {MAX_MIN_SAMPLES}"
                )));
            }
        }

        if !(MIN_PERIOD_SECS..=MAX_PERIOD_SECS).contains(&period_secs) {
            return Err(Error::invalid(format!(
                "a RUM rule is evaluated over {MIN_PERIOD_SECS} to {MAX_PERIOD_SECS} seconds"
            )));
        }

        Ok(())
    }

    /// How many rows the figure needs: the rule's own number, else the metric's.
    pub fn effective_min_samples(&self) -> u32 {
        if self.statistic == Some(RumAlertStatistic::Count)
            || self.search.as_ref().is_some_and(RumAlertSearch::adds_up)
        {
            return 0;
        }

        self.min_samples
            .unwrap_or_else(|| self.metric.default_min_samples())
    }

    /// What it takes of a figure the application measures itself.
    pub fn effective_statistic(&self) -> RumAlertStatistic {
        self.statistic.unwrap_or_default()
    }

    /// The unit its value is in, as rule pages say it.
    pub fn unit(&self) -> &'static str {
        if let Some(search) = &self.search {
            return search.unit();
        }

        if self.effective_statistic() == RumAlertStatistic::Count && self.metric.is_custom() {
            "count"
        } else {
            self.metric.unit()
        }
    }

    /// What the rule watches, for the statement field of rule lists.
    pub fn describe(&self) -> String {
        let mut text = format!("rum:{} {}", self.metric.as_str(), self.application_id);

        if let Some(name) = &self.custom_name {
            text.push(' ');
            text.push_str(name);
        }

        if let Some(page) = &self.page {
            text.push(' ');
            text.push_str(page);
        }

        if let Some(search) = &self.search {
            text.push(' ');
            text.push_str(&search.describe());
        }

        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(metric: RumAlertMetric) -> RumAlertQuery {
        RumAlertQuery {
            application_id: "shop-web".into(),
            environment: Some("prod".into()),
            page: None,
            version: None,
            metric,
            min_samples: None,
            custom_name: None,
            statistic: None,
            search: None,
        }
    }

    #[test]
    fn every_metric_is_listed_once_and_named_by_its_serde_name() {
        for metric in RumAlertMetric::ALL {
            let json = serde_json::to_value(metric).unwrap();

            assert_eq!(json, metric.as_str());
            assert_eq!(
                serde_json::from_value::<RumAlertMetric>(json).unwrap(),
                metric
            );
        }

        let mut names = RumAlertMetric::ALL.map(RumAlertMetric::as_str).to_vec();

        names.sort_unstable();
        names.dedup();

        assert_eq!(names.len(), RumAlertMetric::ALL.len());
    }

    #[test]
    fn only_counts_are_zero_without_rows() {
        let zero = RumAlertMetric::ALL
            .into_iter()
            .filter(|metric| metric.counts_zero())
            .map(RumAlertMetric::as_str)
            .collect::<Vec<_>>();

        assert_eq!(
            zero,
            [
                "session_count",
                "view_count",
                "error_count",
                "new_issues",
                "regressed_issues",
                "issue_impact"
            ]
        );
    }

    #[test]
    fn a_rule_is_narrowed_only_by_what_its_figure_can_be_told_by() {
        let mut sessions = query(RumAlertMetric::CrashFreeSessions);

        assert!(sessions.validate(900).is_ok());

        sessions.page = Some("/checkout".into());
        assert!(sessions.validate(900).is_err());

        let mut views = query(RumAlertMetric::LcpP75);

        views.page = Some("/checkout".into());
        views.version = Some("2.4.1".into());
        assert!(views.validate(900).is_ok());

        let mut issues = query(RumAlertMetric::NewIssues);

        assert!(issues.validate(900).is_ok());

        issues.version = Some("2.4.1".into());
        assert!(issues.validate(900).is_err());

        issues.version = None;
        issues.page = Some("/checkout".into());
        assert!(issues.validate(900).is_err());
    }

    #[test]
    fn names_and_windows_are_bounded() {
        assert!(
            RumAlertQuery {
                application_id: "../shop".into(),
                ..query(RumAlertMetric::ErrorRate)
            }
            .validate(900)
            .is_err()
        );
        assert!(
            RumAlertQuery {
                environment: Some(" ".into()),
                ..query(RumAlertMetric::ErrorRate)
            }
            .validate(900)
            .is_err()
        );
        assert!(query(RumAlertMetric::ErrorRate).validate(30).is_err());
        assert!(
            query(RumAlertMetric::ErrorRate)
                .validate(MAX_PERIOD_SECS)
                .is_ok()
        );
        assert!(
            query(RumAlertMetric::ErrorRate)
                .validate(MAX_PERIOD_SECS + 1)
                .is_err()
        );
    }

    #[test]
    fn a_minimum_of_samples_belongs_to_figures_made_of_rows() {
        let counted = RumAlertQuery {
            min_samples: Some(10),
            ..query(RumAlertMetric::ErrorCount)
        };

        assert!(counted.validate(900).is_err());

        let share = RumAlertQuery {
            min_samples: Some(10),
            ..query(RumAlertMetric::ErrorRate)
        };

        assert!(share.validate(900).is_ok());
        assert_eq!(share.effective_min_samples(), 10);
        assert_eq!(
            query(RumAlertMetric::ErrorRate).effective_min_samples(),
            100
        );
        assert_eq!(query(RumAlertMetric::LcpP75).effective_min_samples(), 50);
    }

    #[test]
    fn a_figure_the_application_measures_is_named() {
        let unnamed = query(RumAlertMetric::CustomVital);

        assert!(unnamed.validate(900).is_err());

        let named = RumAlertQuery {
            custom_name: Some("checkout".into()),
            ..unnamed
        };

        assert!(named.validate(900).is_ok());
        assert_eq!(named.effective_min_samples(), 30);
        assert_eq!(named.unit(), "milliseconds");
        assert_eq!(named.describe(), "rum:custom_vital shop-web checkout");

        let counted = RumAlertQuery {
            statistic: Some(RumAlertStatistic::Count),
            ..named.clone()
        };

        assert!(counted.validate(900).is_ok());
        assert_eq!(counted.effective_min_samples(), 0);
        assert_eq!(counted.unit(), "count");
        assert!(
            RumAlertQuery {
                min_samples: Some(5),
                ..counted
            }
            .validate(900)
            .is_err()
        );

        assert!(
            RumAlertQuery {
                custom_name: Some("x".into()),
                ..query(RumAlertMetric::LcpP75)
            }
            .validate(900)
            .is_err()
        );
        assert!(
            RumAlertQuery {
                statistic: Some(RumAlertStatistic::P95),
                ..query(RumAlertMetric::LcpP75)
            }
            .validate(900)
            .is_err()
        );
    }

    #[test]
    fn describes_what_it_watches() {
        let page = RumAlertQuery {
            page: Some("/checkout".into()),
            ..query(RumAlertMetric::LcpP75)
        };

        assert_eq!(page.describe(), "rum:lcp_p75 shop-web /checkout");
        assert_eq!(
            query(RumAlertMetric::ErrorRate).describe(),
            "rum:error_rate shop-web"
        );
    }

    #[test]
    fn an_absent_narrowing_is_not_written() {
        let json = serde_json::to_value(query(RumAlertMetric::ErrorRate)).unwrap();

        assert_eq!(
            json,
            serde_json::json!({
                "applicationId": "shop-web",
                "environment": "prod",
                "metric": "error_rate"
            })
        );
    }

    fn search(function: &str, field: Option<&str>) -> RumAlertSearch {
        RumAlertSearch {
            kind: "views".into(),
            statement: "browser.name = 'Chrome'".into(),
            filters: vec![RumAlertFilter {
                field: "browser.name".into(),
                op: "=".into(),
                value: "Chrome".into(),
            }],
            free_text: Vec::new(),
            measure: RumAlertMeasure {
                function: function.into(),
                field: field.map(str::to_string),
            },
        }
    }

    #[test]
    fn a_rule_from_the_explorer_watches_the_number_its_query_asks_for() {
        let unsearched = query(RumAlertMetric::Query);

        assert!(unsearched.validate(900).is_err());

        let slow = RumAlertQuery {
            search: Some(search("p75", Some("view.loading_time"))),
            ..unsearched.clone()
        };

        assert!(slow.validate(900).is_ok());
        assert_eq!(slow.unit(), "milliseconds");
        assert_eq!(slow.effective_min_samples(), 30);
        assert_eq!(
            slow.describe(),
            "rum:query shop-web views p75(view.loading_time) browser.name = 'Chrome'"
        );

        // The query names the page and the version among its filters.
        assert!(
            RumAlertQuery {
                page: Some("/checkout".into()),
                ..slow.clone()
            }
            .validate(900)
            .is_err()
        );

        let counted = RumAlertQuery {
            search: Some(search("count", None)),
            ..unsearched
        };

        assert!(counted.validate(900).is_ok());
        assert_eq!(counted.unit(), "count");
        assert_eq!(counted.effective_min_samples(), 0);
        assert!(
            RumAlertQuery {
                min_samples: Some(10),
                ..counted
            }
            .validate(900)
            .is_err()
        );

        assert!(
            RumAlertQuery {
                search: Some(search("count", None)),
                ..query(RumAlertMetric::ErrorRate)
            }
            .validate(900)
            .is_err()
        );
    }
}
