// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow, bail};
use futures::StreamExt as _;
use regex::Regex;
use reqwest::{
    Method, StatusCode, Url,
    header::{HeaderMap, HeaderName, HeaderValue},
};

use super::{AttemptOutcome, ExecutionContext, security::EgressGuard, unknown};
use crate::protocol::v1::{
    self as wire, AssertionResult, AssertionSeverity, BrowserJourneySpec, HttpJourneySpec,
    ProbeOutcome,
};

const MAX_BODY_BYTES: usize = 64 * 1024;

struct HttpObservation {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
    elapsed: Duration,
}

pub(super) async fn execute(
    task: &wire::ProbeTask,
    spec: &HttpJourneySpec,
    context: &mut ExecutionContext,
) -> Result<AttemptOutcome> {
    let guard = EgressGuard::new(task.egress_policy.clone())?;
    let mut assertions = Vec::new();
    let mut last_body = Vec::new();
    let mut metadata = HashMap::new();
    for step in &spec.steps {
        let observation = execute_step(task, spec, step, context, &guard).await?;
        metadata.insert(
            format!("step.{}.status", step.id),
            observation.status.as_u16().to_string(),
        );
        metadata.insert(
            format!("step.{}.duration_micros", step.id),
            observation.elapsed.as_micros().to_string(),
        );
        for extraction in &step.extractions {
            let extracted = extract(extraction, &observation)?;
            if let Some(value) = extracted {
                context.insert_variable(extraction.variable.clone(), value);
            } else if extraction.required {
                bail!(
                    "required extraction `{}` did not match",
                    extraction.variable
                );
            }
        }
        assertions.extend(
            step.assertions
                .iter()
                .map(|assertion| evaluate(assertion, &observation, context))
                .collect::<Result<Vec<_>>>()?,
        );
        last_body = observation.body;
    }
    let critical_failed = assertions
        .iter()
        .any(|result| !result.passed && result.severity == AssertionSeverity::Critical as i32);
    let warning_failed = assertions
        .iter()
        .any(|result| !result.passed && result.severity == AssertionSeverity::Warning as i32);
    let outcome = if critical_failed {
        ProbeOutcome::Failing
    } else if warning_failed {
        ProbeOutcome::Degraded
    } else {
        ProbeOutcome::Healthy
    };
    let excerpt = if matches!(outcome, ProbeOutcome::Failing | ProbeOutcome::Degraded) {
        context
            .redact(&String::from_utf8_lossy(&last_body))
            .into_bytes()
    } else {
        Vec::new()
    };
    Ok(AttemptOutcome {
        outcome,
        assertions,
        response_excerpt: excerpt,
        error_category: String::new(),
        error_message: String::new(),
        metadata,
    })
}

async fn execute_step(
    task: &wire::ProbeTask,
    spec: &HttpJourneySpec,
    step: &wire::HttpStep,
    context: &ExecutionContext,
    guard: &EgressGuard,
) -> Result<HttpObservation> {
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
    let started = Instant::now();
    loop {
        let host = url
            .host_str()
            .ok_or_else(|| anyhow!("HTTP URL has no host"))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| anyhow!("HTTP URL has no port"))?;
        let address = guard.resolve(host, port).await?.remove(0);
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .danger_accept_invalid_certs(!spec.verify_tls)
            .resolve(host, address)
            .build()?;
        let mut request = client
            .request(method.clone(), url.clone())
            .timeout(Duration::from_millis(u64::from(task.timeout_millis.max(1))));
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
                request = request.header(name, value);
            }
            if let Some(body) = &step.body {
                request = request.body(context.resolve(body)?);
            }
        }
        let response = request.send().await?;
        if response.status().is_redirection() && spec.follow_redirects {
            if redirects >= spec.max_redirects {
                bail!("HTTP redirect limit exceeded");
            }
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .ok_or_else(|| anyhow!("HTTP redirect has no Location header"))?
                .to_str()?;
            url = url.join(location)?;
            if response.status() == StatusCode::SEE_OTHER
                || ((response.status() == StatusCode::MOVED_PERMANENTLY
                    || response.status() == StatusCode::FOUND)
                    && method == Method::POST)
            {
                method = Method::GET;
            }
            redirects += 1;
            continue;
        }
        let status = response.status();
        let headers = response.headers().clone();
        let mut stream = response.bytes_stream();
        let mut body = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            let remaining = MAX_BODY_BYTES.saturating_sub(body.len());
            body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
            if body.len() >= MAX_BODY_BYTES {
                break;
            }
        }
        return Ok(HttpObservation {
            status,
            headers,
            body,
            elapsed: started.elapsed(),
        });
    }
}

fn evaluate(
    assertion: &wire::Assertion,
    observation: &HttpObservation,
    context: &ExecutionContext,
) -> Result<AssertionResult> {
    let actual = source_value(&assertion.source, observation)?;
    let expected = assertion
        .expected
        .as_ref()
        .map(|value| context.resolve(value))
        .transpose()?
        .unwrap_or_default();
    let passed = match assertion.operator.as_str() {
        "equals" => actual == expected,
        "not_equals" => actual != expected,
        "contains" => actual.contains(&expected),
        "not_contains" => !actual.contains(&expected),
        "matches" => Regex::new(&expected)?.is_match(&actual),
        "greater_than" => actual.parse::<f64>()? > expected.parse::<f64>()?,
        "less_than" => actual.parse::<f64>()? < expected.parse::<f64>()?,
        "exists" => !actual.is_empty(),
        "json_schema" => {
            serde_json::from_str::<serde_json::Value>(&actual).is_ok()
                && serde_json::from_str::<serde_json::Value>(&expected).is_ok()
        }
        operator => bail!("unsupported HTTP Assertion operator `{operator}`"),
    };
    Ok(AssertionResult {
        assertion_id: assertion.id.clone(),
        severity: assertion.severity,
        passed,
        actual: context.redact(&actual),
        message: if passed {
            String::new()
        } else {
            format!("{} assertion failed", assertion.name)
        },
    })
}

fn source_value(source: &str, observation: &HttpObservation) -> Result<String> {
    match source {
        "status" | "status_code" => Ok(observation.status.as_u16().to_string()),
        "body" => Ok(String::from_utf8_lossy(&observation.body).into_owned()),
        "duration_ms" => Ok(observation.elapsed.as_millis().to_string()),
        source if source.starts_with("header:") => Ok(observation
            .headers
            .get(&source[7..])
            .map(|value| value.to_str().unwrap_or_default().to_string())
            .unwrap_or_default()),
        _ => bail!("unsupported HTTP Assertion source `{source}`"),
    }
}

fn extract(extraction: &wire::Extraction, observation: &HttpObservation) -> Result<Option<String>> {
    let source = source_value(&extraction.source, observation)?;
    let regex = Regex::new(&extraction.expression)?;
    Ok(regex.captures(&source).and_then(|captures| {
        captures
            .get(1)
            .or_else(|| captures.get(0))
            .map(|value| value.as_str().to_string())
    }))
}

pub(super) async fn execute_browser(
    _task: &wire::ProbeTask,
    _spec: &BrowserJourneySpec,
    _context: &mut ExecutionContext,
) -> Result<AttemptOutcome> {
    Ok(unknown(
        "browser_runtime_unavailable",
        "this Probe Agent build does not include the declarative Browser runtime",
    ))
}
