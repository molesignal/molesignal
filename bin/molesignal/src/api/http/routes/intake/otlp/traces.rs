// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! OTLP trace intake: a request's spans enter the tail-sampling queue together or not at all.
//!
//! A full queue refuses the whole request with a retryable status (HTTP 503, gRPC
//! `UNAVAILABLE`), so the sender retries it as it is and nothing is taken twice. Spans that stay
//! over the size limit after sanitising are refused for good and reported in the response's
//! `partial_success`, which senders must not retry.
//!
//! The answer comes only once the tenant spans are stored in the trace candidate log in object
//! storage (`app::trace::durable`), so an acknowledged span outlives this node and its disk.

use axum::{body::Bytes, extract::State, http::HeaderMap, response::Response};
use domain::{
    iam::{IamContext, permission},
    intake::usage::source::IntakeCredential,
    stream::StreamType,
};
use kernel::{Error, Result};
use opentelemetry_proto::tonic::collector::trace::v1::{
    ExportTracePartialSuccess, ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use serde_json::Value;
use signals::trace_normalization::CanonicalSpan;

use super::{
    DEFAULT_STREAM, decode_otlp, detect_encoding, otlp_ok, stream_name, traces_to_canonical,
};
use crate::{
    api::AppState,
    app::{
        intake::usage::{IntakeReceipt, ServiceTally},
        trace::candidate_router::TraceCandidateSubmitError,
    },
};

#[permission("traces.write")]
pub(super) async fn otlp_traces(
    State(state): State<AppState>,
    axum::Extension(ctx): axum::Extension<domain::iam::IamContext>,
    axum::Extension(trace_context): axum::Extension<signals::trace_context::TraceContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response> {
    let enc = detect_encoding(&headers)?;
    let req: ExportTraceServiceRequest = decode_otlp(enc, &body)?;
    let stream = stream_name(&headers, DEFAULT_STREAM);
    let spans = traces_to_canonical(req);

    let submission = submit_traces(
        &state,
        &ctx,
        stream,
        spans,
        body.len(),
        &trace_context,
        headers.contains_key(crate::app::trace::export::SELF_EXPORT_MARKER),
    )
    .await?;

    Ok(otlp_ok(enc, submission.response()))
}

/// What became of a trace request that was not refused as a whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TraceSubmission {
    pub(crate) accepted: usize,
    /// Spans refused for good: still over the size limit after sanitising.
    pub(crate) rejected: usize,
}

impl TraceSubmission {
    /// The OTLP answer: `partial_success` names the refused spans when there are any.
    pub(crate) fn response(self) -> ExportTraceServiceResponse {
        if self.rejected == 0 {
            return ExportTraceServiceResponse::default();
        }

        ExportTraceServiceResponse {
            partial_success: Some(ExportTracePartialSuccess {
                rejected_spans: i64::try_from(self.rejected).unwrap_or(i64::MAX),
                error_message: format!(
                    "{} span(s) exceed the trace candidate size limit after attribute limits \
                     were applied; reduce attribute, event or link sizes",
                    self.rejected
                ),
            }),
        }
    }
}

pub(crate) async fn submit_traces(
    state: &AppState,
    ctx: &IamContext,
    stream: String,
    mut spans: Vec<CanonicalSpan>,
    bytes: usize,
    trace_context: &signals::trace_context::TraceContext,
    suppress_external: bool,
) -> Result<TraceSubmission> {
    // Spans are written after tail sampling, where a refusal reaches no one: a stream only the
    // platform writes is refused here, while the sender waits for the answer.
    domain::stream::refuse_platform_stream(&stream)?;

    let receipt = IntakeReceipt {
        credential: IntakeCredential::of(ctx),
        stream_type: StreamType::TRACES,
        stream: stream.clone(),
        bytes: bytes as u64,
        services: ServiceTally::of_spans(&spans),
    };

    let force_keep = match trace_context.trust {
        signals::trace_context::TraceTrust::DebugToken if trace_context.force_keep => {
            signals::tail_sampling::ForceKeep::DebugToken
        }
        signals::trace_context::TraceTrust::Internal if trace_context.force_keep => {
            signals::tail_sampling::ForceKeep::TrustedInternal
        }
        _ => signals::tail_sampling::ForceKeep::None,
    };

    if suppress_external {
        for span in &mut spans {
            span.attributes.insert(
                "molesignal.trace.suppress_external".into(),
                Value::Bool(true),
            );
        }
    }

    let count = spans.len();

    let candidates = spans
        .into_iter()
        .map(|span| signals::tail_sampling::TraceCandidate {
            org_id: ctx.org_id.0.clone(),
            stream: Some(stream.clone()),
            span,
            force_keep,
        })
        .collect();

    let admission = state
        .telemetry
        .trace_candidates
        .submit_durable(candidates)
        .await
        .map_err(|error| refusal(error, count))?;

    state
        .platform
        .intake_meter
        .record(&ctx.org_id, receipt, admission.accepted as u64);

    Ok(TraceSubmission {
        accepted: admission.accepted,
        rejected: admission.invalid,
    })
}

/// The error a refused request answers with; nothing of it was queued.
fn refusal(error: TraceCandidateSubmitError, count: usize) -> Error {
    match error {
        TraceCandidateSubmitError::Full => Error::unavailable(
            "trace intake queue is full; nothing from this request was accepted, retry it later",
        ),
        TraceCandidateSubmitError::Stopped => {
            Error::unavailable("trace intake is shutting down; retry the request")
        }
        TraceCandidateSubmitError::TooLarge => Error::payload_too_large(format!(
            "the request holds {count} spans, more than the trace intake queue; \
             send smaller batches"
        )),
        TraceCandidateSubmitError::Invalid => {
            Error::invalid("every span exceeds the trace candidate size limit")
        }
        TraceCandidateSubmitError::Unlogged(reason) => {
            tracing::warn!(%reason, "trace spans could not be written to the candidate log");
            Error::unavailable(
                "trace spans could not be made durable; nothing from this request was accepted, \
                 retry it later",
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fully_accepted_request_answers_without_partial_success() {
        let submission = TraceSubmission {
            accepted: 3,
            rejected: 0,
        };

        assert_eq!(submission.response(), ExportTraceServiceResponse::default());
    }

    #[test]
    fn refused_spans_are_reported_as_partial_success() {
        let submission = TraceSubmission {
            accepted: 3,
            rejected: 2,
        };

        let partial = submission
            .response()
            .partial_success
            .expect("partial success");

        assert_eq!(partial.rejected_spans, 2);
        assert!(!partial.error_message.is_empty());
    }

    #[test]
    fn a_full_queue_is_a_retryable_refusal() {
        assert_eq!(
            refusal(TraceCandidateSubmitError::Full, 10).http_status_code(),
            503
        );
        assert_eq!(
            refusal(TraceCandidateSubmitError::TooLarge, 10).http_status_code(),
            413
        );
    }
}
