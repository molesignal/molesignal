// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the model is told of a service attached with `@`: how APM saw it in
//! the time the question is about, the last hour when it names none — each
//! environment's health and figures, the same over the stretch before and a
//! week before, what insights find and the changes near them, and what calls
//! the service and what it calls. Without APM data, the calls the service
//! graph paired over the last day. Then what the service catalog keeps of it.
//! A name APM has no data for that is another name of a service in the
//! catalog is read as that service.

use std::fmt::Write as _;

use domain::iam::IamContext;
use kernel::{
    Result,
    ids::Id,
    time::{TimeRange, TimestampMicros},
};

use crate::{
    api::AppState,
    app::apm::{
        ApmQueryRequest, BaselineFigures, BlastNode, Insight, ServiceSummary, catalog_facts,
    },
};

const HOUR_MICROS: i64 = 3_600_000_000;
const DAY_MICROS: i64 = 24 * HOUR_MICROS;
/// How many insights, callers and calls the model is told of.
const MOST_INSIGHTS: usize = 8;
const MOST_NEIGHBOURS: usize = 6;

pub(super) async fn service_facts(
    state: &AppState,
    ctx: &IamContext,
    name: &str,
    window: Option<TimeRange>,
) -> String {
    let mut facts = apm_facts(state, ctx, name, window).await.ok().flatten();
    let mut service = name.to_owned();

    // Nothing reported under the name: it may be another name of a catalog service.
    if facts.is_none()
        && let Some(canonical) = canonical_name(state, &ctx.org_id, name).await
    {
        let read = apm_facts(state, ctx, &canonical, window)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| format!("APM has no data for {canonical} in the time asked about."));

        facts = Some(format!(
            "\"{name}\" is another name of the service {canonical} in the service catalog.\n{read}"
        ));
        service = canonical;
    }

    let mut text = match facts {
        Some(facts) => facts,
        None => graph_facts(state, &ctx.org_id, &service).await,
    };

    let catalog = catalog_facts(
        state.telemetry.apm_catalog.as_ref(),
        &ctx.org_id,
        std::slice::from_ref(&service),
    )
    .await;

    if let Ok(Some(catalog)) = catalog {
        let _ = write!(text, "\n{catalog}");
    }

    text
}

/// The catalog's service `name` is another name of, when no service of the
/// catalog is called `name`.
async fn canonical_name(state: &AppState, org_id: &Id, name: &str) -> Option<String> {
    let definitions = state
        .telemetry
        .apm_catalog
        .list_definitions(org_id)
        .await
        .ok()?;

    if definitions
        .iter()
        .any(|definition| definition.has_name(name))
    {
        return None;
    }

    definitions
        .iter()
        .filter(|definition| definition.has_alias(name))
        .min_by_key(|definition| definition.archived)
        .map(|definition| definition.name.clone())
}

async fn apm_facts(
    state: &AppState,
    ctx: &IamContext,
    name: &str,
    window: Option<TimeRange>,
) -> Result<Option<String>> {
    let now = TimestampMicros::now().0;
    let range = window.unwrap_or_else(|| {
        TimeRange::new(TimestampMicros(now - HOUR_MICROS), TimestampMicros(now))
    });
    let apm = &state.telemetry.apm_query;
    let request = |service: Option<String>| ApmQueryRequest {
        from: range.start.0,
        to: range.end.0,
        service,
        ..ApmQueryRequest::default()
    };
    let context = apm.context(
        ctx.org_id.clone(),
        request(Some(name.to_owned())),
        &["request_count"],
        "request_count",
    )?;
    let whole = apm.context(
        ctx.org_id.clone(),
        request(None),
        &["request_count"],
        "request_count",
    )?;

    let (services, insights, radius) = tokio::try_join!(
        apm.services(&context),
        apm.insights(&context),
        apm.blast_radius(&whole, state.telemetry.service_graph.as_ref(), name),
    )?;

    if services.items.is_empty() {
        return Ok(None);
    }

    let mut text = format!(
        "Service: {name}, as APM saw it from {} to {}",
        stamp(range.start),
        stamp(range.end)
    );

    for summary in &services.items {
        let _ = write!(text, "\n{}", environment_line(summary));
    }

    for (label, figures) in [
        ("The stretch of the same length before", &insights.previous),
        ("The same stretch a week before", &insights.last_week),
    ] {
        if let Some(figures) = figures {
            let _ = write!(text, "\n{label}: {}", figures_line(figures));
        }
    }

    if !insights.insights.is_empty() {
        text.push_str("\nInsights:");

        for insight in insights.insights.iter().take(MOST_INSIGHTS) {
            let _ = write!(text, "\n- {}", insight_line(insight));
        }
    }

    for (label, nodes) in [
        ("Called by", &radius.upstream),
        ("Calls", &radius.downstream),
    ] {
        if !nodes.is_empty() {
            let named = nodes
                .iter()
                .take(MOST_NEIGHBOURS)
                .map(neighbour)
                .collect::<Vec<_>>()
                .join(", ");
            let _ = write!(text, "\n{label}: {named}");
        }
    }

    Ok(Some(text))
}

fn environment_line(summary: &ServiceSummary) -> String {
    let red = &summary.red;
    let reasons = summary
        .health_reasons
        .iter()
        .map(|reason| reason.metric)
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "- {}/{} in {}: {:?}{}; {} requests, error rate {}, p50 {}, p95 {}, p99 {}{}{}",
        summary.service.namespace,
        summary.service.name,
        summary.service.environment,
        summary.health,
        if reasons.is_empty() {
            String::new()
        } else {
            format!(" ({reasons})")
        },
        red.request_count,
        percent(red.error_rate),
        duration(red.p50_time),
        duration(red.p95_time),
        duration(red.p99_time),
        red.apdex
            .map(|apdex| format!(", Apdex {apdex:.2}"))
            .unwrap_or_default(),
        summary
            .current_version
            .as_ref()
            .map(|version| format!(", version {version}"))
            .unwrap_or_default(),
    )
}

fn figures_line(figures: &BaselineFigures) -> String {
    format!(
        "{} requests ({:.2}/s), error rate {}, p95 {}",
        figures.request_count,
        figures.throughput,
        percent(figures.error_rate),
        duration(figures.p95_time)
    )
}

fn insight_line(insight: &Insight) -> String {
    let subject = insight
        .transaction
        .as_ref()
        .map(|transaction| transaction.name.clone())
        .or_else(|| {
            insight.dependency.as_ref().map(|dependency| {
                format!(
                    "{} {}",
                    dependency.target,
                    dependency.operation.as_deref().unwrap_or_default()
                )
            })
        })
        .unwrap_or_else(|| insight.service.name.clone());
    let figure = |value: f64| match insight.metric {
        Some("error_rate") => percent(value),
        Some("p95") => duration(Some(value as u64)),
        Some(_) => format!("{value:.2}/s"),
        None => format!("{value:.2}"),
    };
    let mut line = format!(
        "{:?} {:?} on {} in {}{}: {}",
        insight.severity,
        insight.kind,
        subject,
        insight.service.environment,
        insight
            .metric
            .map(|metric| format!(" ({metric})"))
            .unwrap_or_default(),
        figure(insight.value)
    );

    for (label, value) in [
        ("before", insight.previous),
        ("a week before", insight.last_week),
    ] {
        if let Some(value) = value {
            let _ = write!(line, ", {label} {}", figure(value));
        }
    }

    if let Some(version) = &insight.version {
        let _ = write!(
            line,
            ", version {version} after {}",
            insight.previous_version.as_deref().unwrap_or("?")
        );
    }

    if let Some(calls) = insight.calls {
        let _ = write!(line, ", {calls} calls");
    }

    if let Some(since) = insight.since {
        let _ = write!(line, ", since {}", stamp(since));
    }

    for change in &insight.changes {
        let _ = write!(
            line,
            "; near then: {:?} of {} {} ({:?}, {:?}) at {}",
            change.kind,
            change.service,
            change.version,
            change.source,
            change.status,
            stamp(change.at)
        );
    }

    line
}

fn neighbour(node: &BlastNode) -> String {
    format!(
        "{} ({:?}, {} hop{}, error rate {})",
        node.name,
        node.health,
        node.hops,
        if node.hops == 1 { "" } else { "s" },
        percent(node.call.error_rate)
    )
}

fn percent(rate: f64) -> String {
    format!("{:.2}%", rate * 100.0)
}

fn duration(micros: Option<u64>) -> String {
    match micros {
        None => "n/a".into(),
        Some(micros) if micros >= 1_000_000 => format!("{:.2} s", micros as f64 / 1_000_000.0),
        Some(micros) => format!("{:.1} ms", micros as f64 / 1_000.0),
    }
}

fn stamp(at: TimestampMicros) -> String {
    at.to_datetime().format("%Y-%m-%d %H:%M UTC").to_string()
}

/// The calls the service graph paired for the service over the last day.
async fn graph_facts(state: &AppState, org_id: &Id, name: &str) -> String {
    let now = TimestampMicros::now().0;

    let edges = state
        .telemetry
        .service_graph
        .query(org_id, now - DAY_MICROS, now, Some(name))
        .await
        .unwrap_or_default();

    let mut lines = vec![format!("Service: {name}")];

    for edge in edges.iter().take(40) {
        lines.push(format!(
            "{} -> {}: {} requests, {} errors in the last day",
            edge.client_service, edge.server_service, edge.request_count, edge.error_count
        ));
    }

    lines.join("\n")
}
