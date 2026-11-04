// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow, bail};
use hyper::{
    Method, StatusCode,
    header::{HeaderName, HeaderValue, LOCATION},
};
use url::{Host, Url};

use super::{
    AttemptOutcome, ExecutionContext, assertions, failure, now_micros,
    timing::PhaseClock,
    trace_context::{self, TRACEPARENT, TraceContext},
};
use crate::protocol::v1::{
    self as wire, AssertionResult, HttpJourneySpec, ProbeOutcome, StepEvidence,
};

mod connection;
mod exchange;
mod proxy;
mod reading;
mod route;
mod tls;

use reading::HttpObservation;
use route::Route;

const MAX_BODY_BYTES: usize = 64 * 1024;

/// How much of a body is kept for JSONPath and JSON Schema, when a step reads it as JSON.
const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;

/// How much of a body is read to tell its size, when a step asserts on it.
const MAX_COUNTED_BYTES: usize = 16 * 1024 * 1024;

pub(super) async fn execute(
    task: &wire::ProbeTask,
    spec: &HttpJourneySpec,
    context: &mut ExecutionContext,
    phases: &PhaseClock,
) -> Result<AttemptOutcome> {
    let route = Route::new(task, spec, context, phases).await?;
    let mut assertions = Vec::new();
    let mut last_body = Vec::new();
    let mut metadata = HashMap::new();
    let mut evidence = Vec::with_capacity(spec.steps.len());

    // The requests of an attempt are spans of one trace, so that the steps of a journey read as
    // one trace in the services they reach.
    let trace = task.inject_trace_context.then(TraceContext::new);

    if let Some(trace) = &trace {
        metadata.insert("trace_id".to_string(), trace.trace_id());
    }

    for step in &spec.steps {
        let step_started = now_micros();
        let traced = trace.as_ref();
        let executed = execute_step(task, spec, step, context, &route, traced, phases).await;

        let observation = match executed {
            Ok(observation) => observation,
            Err(error) => {
                let mut failed = step_failure(
                    step,
                    step_started,
                    classify(&error, context),
                    assertions,
                    excerpt(&last_body, context),
                    metadata,
                    evidence,
                );

                // A request that went out names its trace, whatever became of it.
                if let Some(trace) = trace.filter(|_| !sends_own_traceparent(step))
                    && let Some(evidence) = failed.evidence.last_mut()
                {
                    evidence
                        .metadata
                        .insert("trace_id".into(), trace.trace_id());
                }

                return Ok(failed);
            }
        };

        let mut step_metadata = HashMap::from([
            ("status".into(), observation.status.as_u16().to_string()),
            (
                "duration_micros".into(),
                observation.elapsed.as_micros().to_string(),
            ),
            ("remote_address".into(), observation.remote.to_string()),
            ("http_version".into(), observation.version_text().into()),
        ]);

        if let Some((trace_id, span_id)) = &observation.trace {
            step_metadata.insert("trace_id".into(), trace_id.clone());

            if !span_id.is_empty() {
                step_metadata.insert("span_id".into(), span_id.clone());
            }
        }

        metadata.insert(
            format!("step.{}.status", step.id),
            observation.status.as_u16().to_string(),
        );

        metadata.insert(
            format!("step.{}.remote_address", step.id),
            observation.remote.to_string(),
        );

        metadata.insert(
            format!("step.{}.duration_micros", step.id),
            observation.elapsed.as_micros().to_string(),
        );

        let processed = (|| -> Result<Vec<AssertionResult>> {
            for extraction in &step.extractions {
                let extracted = reading::extract(extraction, &observation)?;

                if let Some(value) = extracted {
                    context.insert_variable(extraction.variable.clone(), value);
                } else if extraction.required {
                    bail!(
                        "required extraction `{}` did not match",
                        extraction.variable
                    );
                }
            }

            assertions::evaluate_all(
                &step.assertions,
                |source| reading::source_value(source, &observation),
                context,
            )
        })();

        let step_assertions = match processed {
            Ok(assertions) => assertions,
            Err(error) => {
                return Ok(step_failure(
                    step,
                    step_started,
                    classify(&error, context),
                    assertions,
                    excerpt(&observation.body, context),
                    metadata,
                    evidence,
                ));
            }
        };

        let step_outcome = assertions::outcome(&step_assertions);

        evidence.push(StepEvidence {
            step_id: step.id.clone(),
            name: step.name.clone(),
            action: "http".into(),
            started_at_micros: step_started,
            finished_at_micros: now_micros(),
            outcome: step_outcome as i32,
            error_category: String::new(),
            error_message: String::new(),
            metadata: step_metadata,
        });

        assertions.extend(step_assertions);
        last_body = observation.body;
    }

    let outcome = assertions::outcome(&assertions);

    let response_excerpt = if matches!(outcome, ProbeOutcome::Failing | ProbeOutcome::Degraded) {
        excerpt(&last_body, context)
    } else {
        Vec::new()
    };

    Ok(AttemptOutcome {
        outcome,
        assertions,
        response_excerpt,
        error_category: String::new(),
        error_message: String::new(),
        metadata,
        evidence,
        artifacts: Vec::new(),
    })
}

/// The start of `body` kept as evidence, with the check's secrets taken out first so that none
/// is cut in two and left half visible.
fn excerpt(body: &[u8], context: &ExecutionContext) -> Vec<u8> {
    let mut text = context.redact(&String::from_utf8_lossy(body));
    let mut end = text.len().min(MAX_BODY_BYTES);

    while !text.is_char_boundary(end) {
        end -= 1;
    }

    text.truncate(end);
    text.into_bytes()
}

/// Whether a step sends a `traceparent` of its own, which then names its trace.
fn sends_own_traceparent(step: &wire::HttpStep) -> bool {
    step.headers
        .iter()
        .any(|header| header.name.trim().eq_ignore_ascii_case(TRACEPARENT))
}

/// How a step that went wrong ends the attempt: failed when the target let it down, unknown when
/// the step could not run.
struct StepFailure {
    outcome: ProbeOutcome,
    category: &'static str,
    message: String,
}

fn classify(error: &anyhow::Error, context: &ExecutionContext) -> StepFailure {
    let message = context.redact(&error.to_string());

    match failure::target_failure(error) {
        Some(failure) => StepFailure {
            outcome: ProbeOutcome::Failing,
            category: failure.category,
            message,
        },
        None => StepFailure {
            outcome: ProbeOutcome::Unknown,
            category: "http_step_failed",
            message,
        },
    }
}

fn step_failure(
    step: &wire::HttpStep,
    started_at: i64,
    failed: StepFailure,
    assertions: Vec<AssertionResult>,
    response_body: Vec<u8>,
    metadata: HashMap<String, String>,
    mut evidence: Vec<StepEvidence>,
) -> AttemptOutcome {
    evidence.push(StepEvidence {
        step_id: step.id.clone(),
        name: step.name.clone(),
        action: "http".into(),
        started_at_micros: started_at,
        finished_at_micros: now_micros(),
        outcome: failed.outcome as i32,
        error_category: failed.category.into(),
        error_message: failed.message.clone(),
        metadata: HashMap::new(),
    });

    AttemptOutcome {
        outcome: failed.outcome,
        assertions,
        response_excerpt: response_body,
        error_category: failed.category.into(),
        error_message: failed.message,
        metadata,
        evidence,
        artifacts: Vec::new(),
    }
}

async fn execute_step(
    task: &wire::ProbeTask,
    spec: &HttpJourneySpec,
    step: &wire::HttpStep,
    context: &ExecutionContext,
    route: &Route,
    trace: Option<&TraceContext>,
    phases: &PhaseClock,
) -> Result<HttpObservation> {
    let before = phases.so_far();
    let own_traceparent = sends_own_traceparent(step);
    let mut traced = None;

    let mut url = Url::parse(
        &context.resolve(
            step.url
                .as_ref()
                .ok_or_else(|| anyhow!("HTTP Step URL is required"))?,
        )?,
    )?;

    if !matches!(url.scheme(), "http" | "https") {
        bail!("HTTP Step URL must use http or https");
    }

    for query in &step.query {
        let value = context.resolve(
            query
                .value
                .as_ref()
                .ok_or_else(|| anyhow!("HTTP query Value is required"))?,
        )?;

        url.query_pairs_mut().append_pair(&query.name, &value);
    }

    let mut method = Method::from_bytes(step.method.as_bytes()).context("invalid HTTP method")?;
    let mut redirects = 0;
    let timeout = Duration::from_millis(u64::from(task.timeout_millis.max(1)));
    let started = Instant::now();

    loop {
        let host = match url.host() {
            Some(Host::Domain(host)) => host.to_string(),
            Some(Host::Ipv4(host)) => host.to_string(),
            Some(Host::Ipv6(host)) => host.to_string(),
            None => return Err(anyhow!("HTTP URL has no host")),
        };

        let port = url
            .port_or_known_default()
            .ok_or_else(|| anyhow!("HTTP URL has no port"))?;

        let address = route.resolve(&host, port, phases).await?.remove(0);
        context.dialling(address.ip());
        let mut request = exchange::request(method.clone(), &url)?;

        if redirects == 0 {
            for header in &step.headers {
                let name = HeaderName::from_bytes(header.name.as_bytes())?;

                let value = HeaderValue::from_str(
                    &context.resolve(
                        header
                            .value
                            .as_ref()
                            .ok_or_else(|| anyhow!("HTTP Header Value is required"))?,
                    )?,
                )?;

                request.headers_mut().append(name, value);
            }

            if let Some(body) = &step.body {
                *request.body_mut() = context.resolve(body)?.into();
            }
        }

        // Every request, redirects too, is a span of the attempt's trace. A step that sends its
        // own traceparent names its trace itself.
        if own_traceparent {
            let own = request.headers().get(TRACEPARENT);

            if let Some(trace_id) = own
                .and_then(|value| value.to_str().ok())
                .and_then(trace_context::trace_id_of)
            {
                traced = Some((trace_id, String::new()));
            }
        } else if let Some(trace) = trace {
            let (span_id, traceparent) = trace.span();

            request
                .headers_mut()
                .insert(TRACEPARENT, HeaderValue::from_str(&traceparent)?);

            traced = Some((trace.trace_id(), span_id));
        }

        // Every request has the whole of the timeout to itself. It goes to the address the
        // egress policy approved, unless the probe's environment puts a proxy in between.
        let (proxies, transport) = (&route.proxies, &route.transport);
        let sent = exchange::send(request, address, proxies, transport, timeout, phases);
        let mut reply = sent.await?;

        if reply.status.is_redirection() && spec.follow_redirects {
            if redirects >= spec.max_redirects {
                bail!("HTTP redirect limit exceeded");
            }

            let location = reply
                .headers
                .get(LOCATION)
                .ok_or_else(|| anyhow!("HTTP redirect has no Location header"))?
                .to_str()?;

            url = url.join(location)?;

            if reply.status == StatusCode::SEE_OTHER
                || ((reply.status == StatusCode::MOVED_PERMANENTLY
                    || reply.status == StatusCode::FOUND)
                    && method == Method::POST)
            {
                method = Method::GET;
            }

            redirects += 1;
            continue;
        }

        let keep = if reading::reads_json(step) {
            MAX_JSON_BYTES
        } else {
            MAX_BODY_BYTES
        };

        let count_up_to = if reading::asks_for_size(step) {
            MAX_COUNTED_BYTES
        } else {
            keep
        };

        let (body, size) = reply.read_body(keep, count_up_to).await?;

        return Ok(HttpObservation {
            status: reply.status,
            version: reply.version,
            headers: reply.headers,
            body,
            size,
            elapsed: started.elapsed(),
            remote: address,
            trace: traced,
            before,
            after: phases.so_far(),
        });
    }
}

#[cfg(test)]
mod tests;
