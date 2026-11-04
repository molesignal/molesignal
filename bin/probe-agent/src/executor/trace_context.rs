// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The W3C trace context (`traceparent`) a check's requests carry, so that the traces of the
//! services they reach lead back to the run: a trace for each attempt, a span for each request.

/// The header, or gRPC metadata key, the context goes out in.
pub(super) const TRACEPARENT: &str = "traceparent";

/// The trace the requests of one attempt belong to.
#[derive(Clone, Copy)]
pub(super) struct TraceContext {
    trace_id: u128,
}

impl TraceContext {
    pub fn new() -> Self {
        // An id of all zeros is not valid; drawing one would take a broken generator.
        Self {
            trace_id: rand::random::<u128>().max(1),
        }
    }

    pub fn trace_id(&self) -> String {
        format!("{:032x}", self.trace_id)
    }

    /// A new span of the trace, for one request: its id, and the `traceparent` that names it.
    pub fn span(&self) -> (String, String) {
        let span_id = format!("{:016x}", rand::random::<u64>().max(1));
        let traceparent = format!("00-{}-{span_id}-01", self.trace_id());
        (span_id, traceparent)
    }
}

/// The trace a `traceparent` the check sets itself names, when it is well formed.
pub(super) fn trace_id_of(traceparent: &str) -> Option<String> {
    let mut fields = traceparent.trim().split('-');
    let version = fields.next()?;
    let trace_id = fields.next()?;
    let span_id = fields.next()?;
    let flags = fields.next()?;

    let lower_hex = |field: &str, length: usize| {
        field.len() == length
            && field
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    };

    let not_zero = |field: &str| field.bytes().any(|byte| byte != b'0');

    // Version 00 has these four fields and no more; a later version may add some.
    let well_formed = lower_hex(version, 2)
        && version != "ff"
        && !(version == "00" && fields.next().is_some())
        && lower_hex(trace_id, 32)
        && not_zero(trace_id)
        && lower_hex(span_id, 16)
        && not_zero(span_id)
        && lower_hex(flags, 2);

    well_formed.then(|| trace_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::{TraceContext, trace_id_of};

    #[test]
    fn every_request_is_a_span_of_the_attempts_trace() {
        let trace = TraceContext::new();
        let (first_span, first) = trace.span();
        let (second_span, second) = trace.span();

        assert_ne!(first_span, second_span);
        assert_eq!(first, format!("00-{}-{first_span}-01", trace.trace_id()));
        assert_eq!(trace_id_of(&first), Some(trace.trace_id()));
        assert_eq!(trace_id_of(&second), Some(trace.trace_id()));
    }

    #[test]
    fn only_a_well_formed_traceparent_names_a_trace() {
        let trace = "4bf92f3577b34da6a3ce929d0e0e4736";

        assert_eq!(
            trace_id_of(&format!("00-{trace}-00f067aa0ba902b7-01")).as_deref(),
            Some(trace)
        );

        for malformed in [
            format!("00-{trace}-00f067aa0ba902b7"),
            format!("00-{trace}-00f067aa0ba902b7-01-extra"),
            format!("00-{}-00f067aa0ba902b7-01", "0".repeat(32)),
            format!("00-{trace}-0000000000000000-01"),
            format!("00-{}-00f067aa0ba902b7-01", trace.to_uppercase()),
            format!("ff-{trace}-00f067aa0ba902b7-01"),
        ] {
            assert_eq!(trace_id_of(&malformed), None, "{malformed}");
        }
    }
}
