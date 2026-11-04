// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The pieces the SQL of the RUM dashboards is made of. The figures are the ones the aggregate
//! queries of the RUM pages compute: a percentile is taken over the views that had a value, and a
//! view or a session counts at its newest revision (`rum_views_latest`, `rum_sessions_latest`).
//! Counts are estimates of what there would be had every session been kept: a row stands for as
//! many as the share it was kept with says.

pub(super) use crate::infra::rum::query::WEIGHT;

/// The value of the environment variable that stands for every environment.
pub(super) const EVERY_ENVIRONMENT: &str = "全部";

/// The rows of the application the dashboard shows, in its environment or in every one, of the
/// sessions people ran.
pub(super) const SCOPE: &str = r#""application.id" = ${application:sqlstring} AND (${environment:sqlstring} = '全部' OR env = ${environment:sqlstring}) AND ("session.type" IS NULL OR "session.type" = 'user')"#;

/// The start of the interval a row falls in, for the charts over time.
pub(super) const BUCKET: &str = "date_bin(INTERVAL '${interval}', _timestamp)";

/// What the rows of a group stand for, as a whole number, and nought where there are none. It is
/// the weight of `query::WEIGHT`, written out so that it can be put into a statement as it is.
pub(super) const TOTAL: &str = r#"CAST(COALESCE(ROUND(SUM(COALESCE(100.0 / NULLIF("retention.rate", 0.0), 1.0))), 0) AS BIGINT)"#;

/// How many sessions the rows of a group were seen in, each counted for the share its rows were
/// kept with.
pub(super) const SESSIONS_SEEN: &str = r#"CAST(COALESCE(ROUND(COUNT(DISTINCT "session.id") * AVG(COALESCE(100.0 / NULLIF("retention.rate", 0.0), 1.0))), 0) AS BIGINT)"#;

/// The 75th percentile of `column` over the rows that have one, divided by `per` to be in the unit
/// the figure is shown in. A row counts for the share it was kept with.
pub(super) fn p75(column: &str, per: &str) -> String {
    format!(
        r#"approx_percentile_cont_with_weight({WEIGHT}, 0.75) WITHIN GROUP (ORDER BY CAST("{column}" AS DOUBLE)) / {per}"#
    )
}

/// The share, in percent, of the rows of a group that meet `condition`: nought where none does.
pub(super) fn share(condition: &str) -> String {
    format!(
        "100.0 * COALESCE(SUM({WEIGHT}) FILTER (WHERE {condition}), 0.0) / NULLIF(SUM({WEIGHT}), 0)"
    )
}

/// The condition on `source` that tells the applications of a platform from the others.
pub(super) fn platform(sources: &[&str]) -> String {
    match sources {
        [one] => format!("source = '{one}'"),
        many => format!(
            "source IN ({})",
            many.iter()
                .map(|source| format!("'{source}'"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// A request that went unanswered or met a server error.
pub(super) const FAILED: &str =
    r#"COALESCE("resource.status_code", 0) = 0 OR "resource.status_code" >= 500"#;

/// What a page is called: its path with the ids in it grouped, else the name the SDK gave it.
pub(super) const PAGE: &str = r#"COALESCE("view.url_path_group", "view.name")"#;

pub(super) const LCP: &str = "view.performance.lcp.timestamp";
pub(super) const INP: &str = "view.performance.inp.duration";
pub(super) const CLS: &str = "view.performance.cls.score";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_totals_are_written_with_the_weight_the_statements_of_the_pages_use() {
        assert!(TOTAL.contains(WEIGHT));
        assert!(SESSIONS_SEEN.contains(WEIGHT));
        assert!(share("x > 0").contains(WEIGHT));
        assert!(p75("view.loading_time", "1").contains(WEIGHT));
    }
}
