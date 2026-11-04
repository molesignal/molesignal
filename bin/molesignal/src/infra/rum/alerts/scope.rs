// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which rows a rule is about: its application, and the environment, version and page it names, of
//! the sessions people ran. A check that fails is told by the check, not by a rule on real users.
//! A rule made from the explorer is about the rows its query matches, as the explorer reads them:
//! a query that asks about the sessions no person ran is taken at its word.

use crate::{
    domain::rum::{RumAlertMetric, RumAlertQuery, RumAlertSearch, RumPlatform},
    infra::rum::query::{
        Condition, Filter, asks_about_traffic, conditions as filter_conditions, ident, literal,
        people_only, text_columns, text_conditions,
    },
    shared::Result,
};

/// Where the page of a view is told by: the path of a web page with its changing parts grouped,
/// or the name of a screen.
pub(super) fn page_field(platform: RumPlatform) -> &'static str {
    if platform == RumPlatform::Browser {
        "view.url_path_group"
    } else {
        "view.name"
    }
}

/// The filters of a query as the explorer sends them.
fn filters_of(search: &RumAlertSearch) -> Vec<Filter> {
    search
        .filters
        .iter()
        .map(|filter| Filter {
            field: filter.field.clone(),
            op: filter.op.clone(),
            value: filter.value.clone(),
            quoted: false,
        })
        .collect()
}

/// The conditions every statement of a rule carries. A name is compared as it is: a `*` in it is
/// a character, not a wildcard.
pub(super) fn conditions(query: &RumAlertQuery, platform: RumPlatform) -> Result<Vec<Condition>> {
    let searched = query
        .search
        .as_ref()
        .map(|search| -> Result<(Vec<Condition>, bool)> {
            let stream = search.stream()?;
            let filters = filters_of(search);
            let mut found = filter_conditions(stream, &filters)?;

            found.extend(text_conditions(text_columns(stream), &search.free_text)?);

            Ok((found, asks_about_traffic(&filters)))
        })
        .transpose()?;

    let mut conditions = vec![Condition::equals("application.id", &query.application_id)];

    // Of the sessions people ran, unless a query asks about the others.
    if !searched.as_ref().is_some_and(|(_, traffic)| *traffic) {
        conditions.push(people_only());
    }

    if let Some(environment) = &query.environment {
        conditions.push(Condition::equals("env", environment));
    }

    if let Some(version) = &query.version {
        conditions.push(Condition::equals("version", version));
    }

    if let Some(page) = &query.page {
        conditions.push(Condition::equals(page_field(platform), page));
    }

    // A figure the application measures is of the rows that have it.
    match (query.metric, &query.custom_name) {
        (RumAlertMetric::CustomVital, Some(name)) => {
            conditions.push(Condition::equals("vital.name", name));
        }
        (RumAlertMetric::CustomTiming, Some(name)) => {
            let column = "view.custom_timings";

            conditions.push(Condition::raw(
                &format!(
                    "json_text({}, {}) IS NOT NULL",
                    ident(column),
                    literal(name)
                ),
                &[column],
                false,
            ));
        }
        _ => {}
    }

    conditions.extend(searched.into_iter().flat_map(|(found, _)| found));

    Ok(conditions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::rum::RumAlertMetric;

    fn query() -> RumAlertQuery {
        RumAlertQuery {
            application_id: "shop-web".into(),
            environment: Some("prod".into()),
            page: Some("/products/*".into()),
            version: Some("2.4.1".into()),
            metric: RumAlertMetric::LcpP75,
            min_samples: None,
            custom_name: None,
            statistic: None,
            search: None,
        }
    }

    #[test]
    fn the_application_is_asked_before_the_latest_revision_and_the_rest_after_it() {
        let found = conditions(&query(), RumPlatform::Browser).unwrap();

        assert_eq!(
            found
                .iter()
                .map(|condition| (condition.columns[0].as_str(), condition.identity))
                .collect::<Vec<_>>(),
            [
                ("application.id", true),
                ("session.type", true),
                ("env", false),
                ("version", false),
                ("view.url_path_group", false),
            ]
        );
    }

    #[test]
    fn a_figure_the_application_measures_reads_the_rows_that_have_it() {
        let vital = conditions(
            &RumAlertQuery {
                metric: RumAlertMetric::CustomVital,
                custom_name: Some("checkout".into()),
                ..query()
            },
            RumPlatform::Browser,
        )
        .unwrap();

        assert_eq!(vital.last().unwrap().sql, "\"vital.name\" = 'checkout'");

        let timing = conditions(
            &RumAlertQuery {
                metric: RumAlertMetric::CustomTiming,
                custom_name: Some("hero".into()),
                ..query()
            },
            RumPlatform::Browser,
        )
        .unwrap();

        assert_eq!(
            timing.last().unwrap().sql,
            "json_text(\"view.custom_timings\", 'hero') IS NOT NULL"
        );
    }

    #[test]
    fn an_app_is_told_by_the_name_of_its_screens() {
        let found = conditions(&query(), RumPlatform::Flutter).unwrap();

        assert_eq!(found.last().unwrap().columns[0], "view.name");
    }

    fn searched(filters: Vec<(&str, &str, &str)>) -> RumAlertQuery {
        RumAlertQuery {
            metric: RumAlertMetric::Query,
            page: None,
            version: None,
            search: Some(RumAlertSearch {
                kind: "views".into(),
                statement: String::new(),
                filters: filters
                    .into_iter()
                    .map(|(field, op, value)| crate::domain::rum::RumAlertFilter {
                        field: field.into(),
                        op: op.into(),
                        value: value.into(),
                    })
                    .collect(),
                free_text: vec!["checkout".into()],
                measure: crate::domain::rum::RumAlertMeasure {
                    function: "count".into(),
                    field: None,
                },
            }),
            ..query()
        }
    }

    #[test]
    fn a_query_reads_the_rows_it_matches_of_the_sessions_people_ran() {
        let found = conditions(
            &searched(vec![("browser.name", "=", "Chrome")]),
            RumPlatform::Browser,
        )
        .unwrap();
        let sql = found
            .iter()
            .map(|condition| condition.sql.as_str())
            .collect::<Vec<_>>();

        assert_eq!(sql[0], "\"application.id\" = 'shop-web'");
        assert_eq!(sql[1], people_only().sql);
        assert_eq!(sql[2], "\"env\" = 'prod'");
        assert!(sql.contains(&"\"browser.name\" = 'Chrome'"), "{sql:?}");
        assert!(
            sql.iter()
                .any(|condition| condition.contains("'%checkout%'")),
            "{sql:?}"
        );

        // A query about the sessions a check ran is taken at its word.
        let checks = conditions(
            &searched(vec![("session.type", "=", "synthetics")]),
            RumPlatform::Browser,
        )
        .unwrap();

        assert!(
            checks
                .iter()
                .all(|condition| condition.sql != people_only().sql)
        );

        // A field the type does not have fails the read.
        assert!(conditions(&searched(vec![("nope", "=", "x")]), RumPlatform::Browser).is_err());
    }

    #[test]
    fn a_star_in_a_name_is_compared_as_it_is() {
        let found = conditions(&query(), RumPlatform::Browser).unwrap();
        let page = found.last().unwrap();

        assert_eq!(page.sql, "\"view.url_path_group\" = '/products/*'");
    }
}
