// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which other signals a service reports besides its traces: logs, metrics
//! and profiles carrying its name, and browser sessions (RUM) whose requests
//! reached it. Each is a quick look over the day before the end of the range
//! in view, kept five minutes. A look that fails or takes too long reads as
//! unknown, never as missing; RUM is never missing either, for a sample of
//! traces can only show that sessions reach a service.

use std::{
    sync::LazyLock,
    time::{Duration, Instant},
};

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use moka::future::Cache;
use serde::Serialize;

use super::QueryParams;
use crate::{
    api::{AppState, http::routes::profiles::profiles_stream},
    app::{apm::record_apm_query_latency, iam::IamContext},
    domain::{
        iam::permission,
        query::{QueryLanguage, QueryRequest, StreamHint},
        rum::event::RumStream,
        stream::{MOLESIGNAL_SYSTEM_STREAM, StreamType},
    },
    infra::query::escape_sql_ident,
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

/// How far back each look goes from the end of the range in view.
const LOOKBACK_MICROS: i64 = 86_400_000_000;
/// Metrics are written every few seconds: an hour of them says enough.
const METRIC_WINDOW: &str = "1h";
const LOOK_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_LOG_STREAMS: usize = 8;
const MAX_RUM_TRACES: usize = 5;
/// How long before and after a trace the browser request that started it may be stamped.
const RESOURCE_GRACE_MICROS: i64 = 5 * 60 * 1_000_000;
/// The field a log stream names its service in, the first the stream has.
const SERVICE_FIELDS: [&str; 3] = ["service.name", "service_name", "service"];

/// Looks kept by service, environment and the five minutes the range ends in.
static COVERAGE: LazyLock<Cache<CoverageKey, ServiceCoverage>> = LazyLock::new(|| {
    Cache::builder()
        .max_capacity(10_000)
        .time_to_live(Duration::from_secs(300))
        .build()
});

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CoverageKey {
    org_id: Id,
    /// The platform scope reads the platform's own streams, which others do not see.
    system_scope: bool,
    namespace: String,
    name: String,
    environment: String,
    slot: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CoverageState {
    Reported,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
struct SignalCoverage {
    state: CoverageState,
    /// Where it was found: log streams, profile types, a RUM session.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    found: Vec<String>,
}

impl SignalCoverage {
    fn of(found: Vec<String>) -> Self {
        let state = if found.is_empty() {
            CoverageState::Missing
        } else {
            CoverageState::Reported
        };

        Self { state, found }
    }

    fn unknown() -> Self {
        Self {
            state: CoverageState::Unknown,
            found: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ServiceCoverage {
    from: TimestampMicros,
    to: TimestampMicros,
    logs: SignalCoverage,
    metrics: SignalCoverage,
    profiles: SignalCoverage,
    rum: SignalCoverage,
}

/// The signals a service reports beside its traces, over the day before `to`.
#[permission(any("apm.services.read", "sys.telemetry.read"))]
pub(super) async fn service_coverage(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(service): Path<String>,
    Query(params): Query<QueryParams>,
) -> Result<Json<ServiceCoverage>> {
    let started = Instant::now();
    let to = params.to.unwrap_or_else(|| TimestampMicros::now().0);
    let range = TimeRange::new(TimestampMicros(to - LOOKBACK_MICROS), TimestampMicros(to));
    let namespace = params.namespace.clone().unwrap_or_default();
    let environment = params.environment.clone().unwrap_or_default();

    if service.trim().is_empty() || namespace.is_empty() || environment.is_empty() {
        return Err(Error::invalid(
            "a service's coverage needs its namespace and environment",
        ));
    }

    // The service in its environment over the day, whatever else the page has in view.
    let scope = QueryParams {
        from: Some(range.start.0),
        to: Some(to),
        namespace: Some(namespace.clone()),
        service: Some(service.clone()),
        environment: Some(environment.clone()),
        ..QueryParams::default()
    };
    // The platform's own telemetry only for the platform scope in its own organization, as the
    // metric catalog has it.
    let self_telemetry = ctx.is_system_scope() && ctx.org_id == state.iam.system_org_id;
    let key = CoverageKey {
        org_id: ctx.org_id.clone(),
        system_scope: ctx.is_system_scope(),
        namespace,
        name: service.clone(),
        environment,
        slot: to / 300_000_000,
    };

    if let Some(kept) = COVERAGE.get(&key).await {
        return Ok(Json(kept));
    }

    let (logs, metrics, profiles, rum) = tokio::join!(
        look(logs_of(
            &state,
            &ctx.org_id,
            self_telemetry,
            &service,
            range
        )),
        look(metrics_of(&state, &ctx.org_id, &service, range)),
        look(profiles_of(&state, &ctx, &service, range)),
        look(rum_of(&state, &ctx, &scope)),
    );

    let coverage = ServiceCoverage {
        from: range.start,
        to: range.end,
        logs,
        metrics,
        profiles,
        rum,
    };

    COVERAGE.insert(key, coverage.clone()).await;
    record_apm_query_latency("coverage", started.elapsed());
    Ok(Json(coverage))
}

/// A look within its time; unknown when it fails or runs out of time.
async fn look(check: impl Future<Output = Result<SignalCoverage>>) -> SignalCoverage {
    match tokio::time::timeout(LOOK_TIMEOUT, check).await {
        Ok(Ok(coverage)) => coverage,
        Ok(Err(error)) => {
            tracing::debug!(%error, "APM coverage look failed");
            SignalCoverage::unknown()
        }
        Err(_) => SignalCoverage::unknown(),
    }
}

/// The log streams holding a line of the service: `default` first, a few at most.
async fn logs_of(
    state: &AppState,
    org_id: &Id,
    self_telemetry: bool,
    name: &str,
    range: TimeRange,
) -> Result<SignalCoverage> {
    let mut streams = state
        .telemetry
        .streams
        .list(org_id)
        .await?
        .into_iter()
        .filter(|stream| stream.stream_type == StreamType::LOGS)
        .filter(|stream| self_telemetry || stream.name != MOLESIGNAL_SYSTEM_STREAM)
        .filter_map(|stream| {
            let field = SERVICE_FIELDS.into_iter().find(|wanted| {
                stream
                    .schema
                    .fields
                    .iter()
                    .any(|field| field.name == *wanted)
            })?;
            Some((stream.name, field))
        })
        .collect::<Vec<_>>();

    streams.sort_by(|left, right| {
        (left.0 != "default", &left.0).cmp(&(right.0 != "default", &right.0))
    });
    streams.truncate(MAX_LOG_STREAMS);

    let looks = streams.iter().map(|(stream, field)| async move {
        let field = escape_sql_ident(field);
        let statement = format!(
            "SELECT \"{field}\" FROM \"{}\" WHERE \"{field}\" = '{}' LIMIT 1",
            escape_sql_ident(stream),
            name.replace('\'', "''"),
        );
        let found = run(
            state,
            org_id,
            (QueryLanguage::Sql, statement, 1),
            range,
            Some((stream, StreamType::LOGS)),
        )
        .await?;

        Ok::<_, Error>((!found.rows.is_empty()).then(|| stream.clone()))
    });

    let results = futures::future::join_all(looks).await;
    let found = results
        .iter()
        .filter_map(|result| result.as_ref().ok().cloned().flatten())
        .collect::<Vec<_>>();

    // Nothing found while a look failed: it may be in the stream that could not be read.
    if found.is_empty() && results.iter().any(Result::is_err) {
        return Ok(SignalCoverage::unknown());
    }

    Ok(SignalCoverage::of(found))
}

/// Whether any series carries the service's name in the last hour of the day.
async fn metrics_of(
    state: &AppState,
    org_id: &Id,
    name: &str,
    range: TimeRange,
) -> Result<SignalCoverage> {
    let selector = format!(
        "{{\"service.name\"=\"{}\"}}",
        name.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let statement = format!("count(last_over_time({selector}[{METRIC_WINDOW}]))");
    // Without a step PromQL answers the figure at the end of the range, not a series
    // over it; the count is a single row.
    let found = run(
        state,
        org_id,
        (QueryLanguage::Promql, statement, 1),
        range,
        None,
    )
    .await?;
    let value = found.columns.iter().position(|column| column == "value");
    let series = found
        .rows
        .iter()
        .filter_map(|row| value.and_then(|index| row.get(index)))
        .filter_map(serde_json::Value::as_f64)
        .sum::<f64>();

    Ok(SignalCoverage::of(if series > 0.0 {
        vec![format!("{series}")]
    } else {
        Vec::new()
    }))
}

/// The kinds of profile the service sent.
async fn profiles_of(
    state: &AppState,
    ctx: &IamContext,
    name: &str,
    range: TimeRange,
) -> Result<SignalCoverage> {
    let stream = profiles_stream(ctx.scope);
    let definition = match state
        .telemetry
        .streams
        .get(&ctx.org_id, stream, StreamType::PROFILES)
        .await
    {
        Ok(definition) => definition,
        Err(Error::NotFound(_)) => return Ok(SignalCoverage::of(Vec::new())),
        Err(error) => return Err(error),
    };
    let has = |column: &str| {
        definition
            .schema
            .fields
            .iter()
            .any(|field| field.name == column)
    };

    if !has("service") || !has("profile_type") {
        return Ok(SignalCoverage::of(Vec::new()));
    }

    let statement = format!(
        "SELECT DISTINCT profile_type FROM \"{}\" WHERE service = '{}' LIMIT 10",
        escape_sql_ident(stream),
        name.replace('\'', "''"),
    );
    let found = run(
        state,
        &ctx.org_id,
        (QueryLanguage::Sql, statement, 10),
        range,
        Some((stream, StreamType::PROFILES)),
    )
    .await?;
    let mut types = found
        .rows
        .iter()
        .filter_map(|row| {
            row.first()
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();

    types.sort();
    Ok(SignalCoverage::of(types))
}

/// A browser session one of the service's sample traces came from: the
/// session of the browser request that started it.
async fn rum_of(
    state: &AppState,
    ctx: &IamContext,
    params: &QueryParams,
) -> Result<SignalCoverage> {
    let context = state.telemetry.apm_query.context(
        ctx.org_id.clone(),
        params.request(),
        &["request_count"],
        "request_count",
    )?;
    let exemplars = state
        .telemetry
        .apm_query
        .recent_exemplars(&context, MAX_RUM_TRACES)
        .await?;
    let stream = RumStream::Resources.name();

    // A trace id is hex; anything else cannot have come from a browser request.
    for exemplar in exemplars.iter().filter(|exemplar| {
        !exemplar.trace_id.is_empty()
            && exemplar
                .trace_id
                .chars()
                .all(|char| char.is_ascii_hexdigit())
    }) {
        let around = TimeRange::new(
            TimestampMicros(exemplar.event_time.0 - RESOURCE_GRACE_MICROS),
            TimestampMicros(exemplar.event_time.0 + RESOURCE_GRACE_MICROS),
        );
        let statement = format!(
            r#"SELECT "{}" FROM "{}" WHERE "{}" = '{}' LIMIT 1"#,
            escape_sql_ident("session.id"),
            escape_sql_ident(stream),
            escape_sql_ident("resource.trace_id"),
            exemplar.trace_id,
        );
        let found = run(
            state,
            &ctx.org_id,
            (QueryLanguage::Sql, statement, 1),
            around,
            Some((stream, StreamType::LOGS)),
        )
        .await?;

        if let Some(session) = found
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(|value| value.as_str())
            .filter(|session| !session.is_empty())
        {
            return Ok(SignalCoverage::of(vec![session.to_owned()]));
        }
    }

    Ok(SignalCoverage::unknown())
}

/// A statement in its language with the most rows it may answer.
type Statement = (QueryLanguage, String, usize);

async fn run(
    state: &AppState,
    org_id: &Id,
    (language, statement, limit): Statement,
    range: TimeRange,
    stream: Option<(&str, StreamType)>,
) -> Result<crate::domain::query::QueryResult> {
    state
        .query
        .run(QueryRequest {
            org_id: org_id.clone(),
            language,
            statement,
            time_range: range,
            stream: stream.map(|(name, stream_type)| StreamHint {
                name: name.to_owned(),
                stream_type,
            }),
            limit: Some(limit),
            step: None,
            federation_clusters: Vec::new(),
        })
        .await
}
