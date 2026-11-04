// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The RUM figures alert rules watch, read the way the pages read them.
//!
//! Every read goes through [`Plan`], the aggregate the overview and the explorer use, over the
//! streams as they are written: a view or a session counts at its latest revision. What a stream
//! was never written is no data, not a failure. The reads of one scope and one window are shared
//! for a short while, so the rules and the previews of an application do not each scan its rows.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use moka::sync::Cache;

use super::{
    groups::{Group, IMPACT, ISSUE_FIELD, ISSUES, Numbers, figure_of, measures_of},
    samples::{self, SAMPLES},
    scope,
};
use crate::{
    domain::{
        query::{QueryEngine, QueryLanguage, QueryRequest, QueryResult, StreamHint},
        rum::{
            RumAlertMetric, RumAlertPoint, RumAlertQuery, RumAlertReading, RumAlertSample,
            RumAlertSearch, RumAlertSource, RumAlertStatistic, RumApplicationRepository,
            RumIssueMoment, RumIssueRepository, RumPlatform, event::RumStream,
        },
        stream::StreamType,
    },
    infra::rum::query::{AggregateSpec, BucketPlan, MeasureSpec, Plan, Scope},
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

/// How long the read of a window that is still going on is shared.
const RECENT_FOR: Duration = Duration::from_secs(30);

/// A window that ended more than this long ago is settled: its read is shared for longer, and the
/// window is cut to a grid so that the reads a rule makes on every evaluation (the same time on
/// the days before) are the same read.
const SETTLED_AFTER_MICROS: i64 = 60 * 60 * 1_000_000;
const SETTLED_FOR: Duration = Duration::from_secs(15 * 60);
const GRID_MICROS: i64 = 5 * 60 * 1_000_000;

/// How long the platform of an application is remembered.
const PLATFORM_FOR: Duration = Duration::from_secs(60);

/// The most issues one read counts. A window with more is counted as this many.
const MAX_ISSUES: usize = 10_000;

/// The issues an incident of new or regressed issues is told by.
const ISSUES_SHOWN: usize = 20;

/// The issues followed through the bars of a preview of the worst issue.
const IMPACT_ISSUES_FOLLOWED: usize = 20;

#[derive(Clone, PartialEq, Eq, Hash)]
struct ReadKey {
    org: Id,
    group: Group,
    /// The figure the application measures itself that the read is of, and what is taken of it.
    custom: Option<(String, RumAlertStatistic)>,
    /// The explorer query the read is of.
    search: Option<RumAlertSearch>,
    application: String,
    environment: Option<String>,
    version: Option<String>,
    page: Option<String>,
    from: i64,
    to: i64,
}

pub struct RumAlerts {
    engine: Arc<dyn QueryEngine>,
    applications: Arc<dyn RumApplicationRepository>,
    issues: Arc<dyn RumIssueRepository>,
    platforms: Cache<(Id, String), Option<RumPlatform>>,
    recent: Cache<ReadKey, Arc<Numbers>>,
    settled: Cache<ReadKey, Arc<Numbers>>,
}

impl RumAlerts {
    pub fn new(
        engine: Arc<dyn QueryEngine>,
        applications: Arc<dyn RumApplicationRepository>,
        issues: Arc<dyn RumIssueRepository>,
    ) -> Self {
        Self {
            engine,
            applications,
            issues,
            platforms: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(PLATFORM_FOR)
                .build(),
            recent: Cache::builder()
                .max_capacity(2_000)
                .time_to_live(RECENT_FOR)
                .build(),
            settled: Cache::builder()
                .max_capacity(4_000)
                .time_to_live(SETTLED_FOR)
                .build(),
        }
    }

    /// What kind of application it is, which tells how its pages are named; none when there is no
    /// such application.
    async fn platform(&self, org_id: &Id, application_id: &str) -> Result<Option<RumPlatform>> {
        let key = (org_id.clone(), application_id.to_string());

        if let Some(found) = self.platforms.get(&key) {
            return Ok(found);
        }

        let platform = match self.applications.get(org_id, application_id).await {
            Ok(application) => Some(application.platform),
            Err(Error::NotFound(_)) => None,
            Err(error) => return Err(error),
        };

        self.platforms.insert(key, platform);

        Ok(platform)
    }

    /// The numbers of the read a figure is made of, over `[from, to)`.
    async fn numbers(
        &self,
        org_id: &Id,
        query: &RumAlertQuery,
        platform: RumPlatform,
        (from, to): (i64, i64),
    ) -> Result<Arc<Numbers>> {
        let group = Group::of(query);
        let now = TimestampMicros::now().0;

        let (from, to, cache) = if to <= now - SETTLED_AFTER_MICROS {
            let end = to - to.rem_euclid(GRID_MICROS);

            (end - (to - from), end, &self.settled)
        } else {
            (from, to, &self.recent)
        };

        let key = ReadKey {
            org: org_id.clone(),
            group,
            custom: query
                .custom_name
                .clone()
                .map(|name| (name, query.effective_statistic())),
            search: query.search.clone(),
            application: query.application_id.clone(),
            environment: query.environment.clone(),
            version: query.version.clone(),
            page: query.page.clone(),
            from,
            to,
        };

        if let Some(found) = cache.get(&key) {
            return Ok(found);
        }

        let numbers = Arc::new(
            self.compute(org_id, query, platform, group, (from, to))
                .await?,
        );

        cache.insert(key, numbers.clone());

        Ok(numbers)
    }

    /// Asks for the numbers of a group, whatever is shared.
    async fn compute(
        &self,
        org_id: &Id,
        query: &RumAlertQuery,
        platform: RumPlatform,
        group: Group,
        (from, to): (i64, i64),
    ) -> Result<Numbers> {
        let Some(stream) = group.stream() else {
            let moments = self
                .moments(org_id, query, group, (from, to), MAX_ISSUES)
                .await?;

            return Ok(Numbers::only(ISSUES, moments.len() as f64));
        };

        let measures = measures_of(query);
        let by_issue = [ISSUE_FIELD.to_string()];

        let plan = if group == Group::IssueImpact {
            Self::by_issue(stream, &measures, &by_issue, None, 1)?
        } else {
            Plan::new(
                stream,
                &AggregateSpec {
                    measures: &measures,
                    group_by: &[],
                    bars: None,
                    order_by: None,
                    descending: None,
                    limit: None,
                },
            )?
        };

        let conditions = scope::conditions(query, platform)?;

        let statement = plan.first_sql(&Scope {
            stream,
            from,
            to,
            conditions: &conditions,
        });

        match self.run(org_id, stream, (from, to), statement).await? {
            Some(result) => Ok(Numbers::of(&plan.answer(&result.columns, &result.rows)?)),
            None => Ok(Numbers::default()),
        }
    }

    /// The read of the issues that touched the most sessions, the worst first: it ranks by the
    /// first measure, which is that.
    fn by_issue(
        stream: RumStream,
        measures: &[MeasureSpec],
        group_by: &[String],
        bars: Option<BucketPlan>,
        limit: usize,
    ) -> Result<Plan> {
        Plan::new(
            stream,
            &AggregateSpec {
                measures,
                group_by,
                bars,
                order_by: None,
                descending: None,
                limit: Some(limit),
            },
        )
    }

    /// The issue that touched the most sessions in a window, for an incident to be told by.
    async fn worst_issue(
        &self,
        org_id: &Id,
        query: &RumAlertQuery,
        platform: RumPlatform,
        (from, to): (i64, i64),
    ) -> Result<Option<String>> {
        let measures = measures_of(query);
        let by_issue = [ISSUE_FIELD.to_string()];
        let stream = RumStream::Errors;
        let plan = Self::by_issue(stream, &measures, &by_issue, None, 1)?;
        let conditions = scope::conditions(query, platform)?;

        let statement = plan.first_sql(&Scope {
            stream,
            from,
            to,
            conditions: &conditions,
        });

        let Some(result) = self.run(org_id, stream, (from, to), statement).await? else {
            return Ok(None);
        };

        Ok(plan
            .groups_of(&result.columns, &result.rows)?
            .into_iter()
            .next()
            .and_then(|group| group.into_iter().next().flatten()))
    }

    /// The worst issue bar by bar: of the issues that touched the most sessions over the range, the
    /// one that touched the most in each bar.
    async fn impact_by_bar(
        &self,
        org_id: &Id,
        query: &RumAlertQuery,
        platform: RumPlatform,
        bars: BucketPlan,
        (from, to): (i64, i64),
    ) -> Result<Vec<(i64, f64)>> {
        let measures = measures_of(query);
        let by_issue = [ISSUE_FIELD.to_string()];
        let stream = RumStream::Errors;
        let plan = Self::by_issue(
            stream,
            &measures,
            &by_issue,
            Some(bars),
            IMPACT_ISSUES_FOLLOWED,
        )?;
        let conditions = scope::conditions(query, platform)?;

        let scope = Scope {
            stream,
            from,
            to,
            conditions: &conditions,
        };

        let Some(ranked) = self
            .run(org_id, stream, (from, to), plan.first_sql(&scope))
            .await?
        else {
            return Ok(Vec::new());
        };

        let issues = plan.groups_of(&ranked.columns, &ranked.rows)?;

        if issues.is_empty() {
            return Ok(Vec::new());
        }

        let Some(result) = self
            .run(org_id, stream, (from, to), plan.series_sql(&scope, &issues))
            .await?
        else {
            return Ok(Vec::new());
        };

        Ok(plan
            .answer(&result.columns, &result.rows)?
            .rows
            .iter()
            .filter_map(|row| Some((row.start_time?, row.values.first()?.as_f64()?)))
            .collect())
    }

    /// The issues new or back in a window, for the figures about issues.
    async fn moments(
        &self,
        org_id: &Id,
        query: &RumAlertQuery,
        group: Group,
        (from, to): (i64, i64),
        limit: usize,
    ) -> Result<Vec<RumIssueMoment>> {
        let (from, to) = (TimestampMicros(from), TimestampMicros(to));
        let environment = query.environment.as_deref();

        if group == Group::RegressedIssues {
            self.issues
                .regressed_between(org_id, &query.application_id, environment, from, to, limit)
                .await
        } else {
            self.issues
                .first_seen_between(org_id, &query.application_id, environment, from, to, limit)
                .await
        }
    }

    /// Runs a statement over a stream; none when nothing was ever written to the stream.
    async fn run(
        &self,
        org_id: &Id,
        stream: RumStream,
        (from, to): (i64, i64),
        statement: String,
    ) -> Result<Option<QueryResult>> {
        let request = QueryRequest {
            org_id: org_id.clone(),
            language: QueryLanguage::Sql,
            statement,
            time_range: TimeRange::new(TimestampMicros(from), TimestampMicros(to)),
            stream: Some(StreamHint {
                name: stream.name().to_string(),
                stream_type: StreamType::LOGS,
            }),
            limit: None,
            step: None,
            federation_clusters: Vec::new(),
        };

        match self.engine.execute(request).await {
            Ok(result) => Ok(Some(result)),
            Err(error) if is_missing_stream(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

/// Whether the engine refused because the stream does not exist yet.
fn is_missing_stream(error: &Error) -> bool {
    matches!(error, Error::NotFound(_))
        || error
            .to_string()
            .to_ascii_lowercase()
            .contains("stream not found")
}

#[async_trait]
impl RumAlertSource for RumAlerts {
    async fn read(
        &self,
        org_id: &Id,
        query: &RumAlertQuery,
        range: TimeRange,
    ) -> Result<Option<RumAlertReading>> {
        let Some(platform) = self.platform(org_id, &query.application_id).await? else {
            // The application is gone: there is nothing to watch, and no threshold matches.
            return Ok(None);
        };

        let numbers = self
            .numbers(org_id, query, platform, (range.start.0, range.end.0))
            .await?;

        Ok(figure_of(query, &numbers))
    }

    async fn samples(
        &self,
        org_id: &Id,
        query: &RumAlertQuery,
        range: TimeRange,
    ) -> Result<Vec<RumAlertSample>> {
        let Some(platform) = self.platform(org_id, &query.application_id).await? else {
            return Ok(Vec::new());
        };

        let window = (range.start.0, range.end.0);

        let issues = if query.metric.is_about_issues() {
            self.moments(org_id, query, Group::of(query), window, ISSUES_SHOWN)
                .await?
                .into_iter()
                .map(|moment| moment.issue_id)
                .collect()
        } else if query.metric == RumAlertMetric::IssueImpact {
            self.worst_issue(org_id, query, platform, window)
                .await?
                .into_iter()
                .collect()
        } else {
            Vec::new()
        };

        let conditions = scope::conditions(query, platform)?;

        let found = match &query.search {
            Some(search) => Some(samples::searched(search.stream()?, &conditions, window)),
            None => samples::statement(query.metric, &conditions, window, &issues)?,
        };

        let Some(statement) = found else {
            return Ok(Vec::new());
        };

        let Some(result) = self
            .run(org_id, statement.stream, window, statement.sql.clone())
            .await?
        else {
            return Ok(Vec::new());
        };

        let mut found = samples::samples_of(&statement, &result);

        found.truncate(SAMPLES);

        Ok(found)
    }

    async fn series(
        &self,
        org_id: &Id,
        query: &RumAlertQuery,
        range: TimeRange,
        bar_micros: i64,
    ) -> Result<Vec<RumAlertPoint>> {
        let Some(platform) = self.platform(org_id, &query.application_id).await? else {
            return Err(Error::not_found(format!(
                "RUM application `{}` does not exist",
                query.application_id
            )));
        };

        let (from, to) = (range.start.0, range.end.0);
        let bars = BucketPlan::new(Some(from), Some(to), Some(bar_micros), None)?;
        let count = (to - from + bar_micros - 1) / bar_micros;
        let group = Group::of(query);

        let point = |index: i64, numbers: &Numbers| RumAlertPoint {
            at: TimestampMicros(from + index * bar_micros),
            reading: figure_of(query, numbers),
        };

        let Some(stream) = group.stream() else {
            // The issues are counted by the bar their moment falls in.
            let moments = self
                .moments(org_id, query, group, (from, to), MAX_ISSUES)
                .await?;

            let mut counts = vec![0_u32; usize::try_from(count).unwrap_or_default()];

            for moment in moments {
                let index = usize::try_from((moment.at.0 - from) / bar_micros).unwrap_or(0);

                if let Some(slot) = counts.get_mut(index) {
                    *slot += 1;
                }
            }

            return Ok(counts
                .iter()
                .enumerate()
                .map(|(index, count)| {
                    point(index as i64, &Numbers::only(ISSUES, f64::from(*count)))
                })
                .collect());
        };

        if group == Group::IssueImpact {
            let found = self
                .impact_by_bar(org_id, query, platform, bars, (from, to))
                .await?;

            return Ok((0..count)
                .map(|index| {
                    let start = from + index * bar_micros;

                    let worst = found
                        .iter()
                        .filter(|(at, _)| *at == start)
                        .map(|(_, sessions)| *sessions)
                        .fold(0.0, f64::max);

                    point(index, &Numbers::only(IMPACT, worst))
                })
                .collect());
        }

        let measures = measures_of(query);

        let plan = Plan::new(
            stream,
            &AggregateSpec {
                measures: &measures,
                group_by: &[],
                bars: Some(bars),
                order_by: None,
                descending: None,
                limit: None,
            },
        )?;

        let conditions = scope::conditions(query, platform)?;

        let statement = plan.first_sql(&Scope {
            stream,
            from,
            to,
            conditions: &conditions,
        });

        let aggregate = match self.run(org_id, stream, (from, to), statement).await? {
            Some(result) => plan.answer(&result.columns, &result.rows)?,
            None => plan.empty(),
        };

        Ok((0..count)
            .map(|index| {
                let start = from + index * bar_micros;

                let numbers = aggregate
                    .rows
                    .iter()
                    .position(|row| row.start_time == Some(start))
                    .map(|row| Numbers::of_row(&aggregate, row))
                    .unwrap_or_default();

                point(index, &numbers)
            })
            .collect())
    }
}

#[cfg(test)]
mod tests;
