// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What the reply to an HTTP step offers its assertions and extractions.

use std::{net::SocketAddr, time::Duration};

use anyhow::{Result, anyhow};
use hyper::{StatusCode, Version, header::HeaderMap};
use regex::Regex;
use serde_json::Value;
use serde_json_path::JsonPath;

use crate::{
    executor::timing::{Phase, Readings, spent},
    protocol::v1 as wire,
};

/// What came back for one step: the last response, after any redirects it followed.
pub(super) struct HttpObservation {
    pub status: StatusCode,
    pub version: Version,
    pub headers: HeaderMap,
    /// The start of the body: an excerpt, or as much as JSON is read from when the step reads
    /// it as JSON.
    pub body: Vec<u8>,
    /// How many bytes of body were read, all of them when a size is asserted on.
    pub size: usize,
    pub elapsed: Duration,
    /// The address the last request went to.
    pub remote: SocketAddr,
    /// The trace the step's requests carried, and the span of the last one: empty when the step
    /// sent its own traceparent.
    pub trace: Option<(String, String)>,
    /// The share of each phase this step took.
    pub before: Readings,
    pub after: Readings,
}

impl HttpObservation {
    /// The HTTP version the server answered in, as tools write it.
    pub fn version_text(&self) -> &'static str {
        match self.version {
            Version::HTTP_09 => "HTTP/0.9",
            Version::HTTP_10 => "HTTP/1.0",
            Version::HTTP_2 => "HTTP/2",
            Version::HTTP_3 => "HTTP/3",
            _ => "HTTP/1.1",
        }
    }

    fn phase_millis(&self, phase: Phase) -> String {
        spent(&self.before, &self.after, phase)
            .unwrap_or_default()
            .as_millis()
            .to_string()
    }
}

/// Whether a step asks how big its response body is, so that it has to be read to the end.
pub(super) fn asks_for_size(step: &wire::HttpStep) -> bool {
    step.assertions
        .iter()
        .any(|assertion| assertion.source == "size_bytes")
        || step
            .extractions
            .iter()
            .any(|extraction| extraction.source == "size_bytes")
}

/// Whether a step reads its body as JSON, so that it keeps more of the body than an excerpt.
pub(super) fn reads_json(step: &wire::HttpStep) -> bool {
    step.assertions.iter().any(|assertion| {
        assertion.source.starts_with("json:") || assertion.operator == "json_schema"
    }) || step
        .extractions
        .iter()
        .any(|extraction| extraction.source.starts_with("json:"))
}

/// The value of `source` in the reply, or `None` for a source an HTTP step does not offer.
pub(super) fn source_value(source: &str, observation: &HttpObservation) -> Option<String> {
    match source {
        "status" | "status_code" => Some(observation.status.as_u16().to_string()),
        "body" => Some(String::from_utf8_lossy(&observation.body).into_owned()),
        "duration_ms" => Some(observation.elapsed.as_millis().to_string()),
        "size_bytes" => Some(observation.size.to_string()),
        "timing.dns_ms" => Some(observation.phase_millis(Phase::Dns)),
        "timing.connect_ms" => Some(observation.phase_millis(Phase::Connect)),
        "timing.tls_ms" => Some(observation.phase_millis(Phase::Tls)),
        "timing.first_byte_ms" => Some(observation.phase_millis(Phase::FirstByte)),
        source => {
            if let Some(name) = source.strip_prefix("header:") {
                Some(
                    observation
                        .headers
                        .get(name)
                        .map(|value| value.to_str().unwrap_or_default().to_string())
                        .unwrap_or_default(),
                )
            } else {
                source
                    .strip_prefix("json:")
                    .map(|path| json_value(&observation.body, path))
            }
        }
    }
}

/// The value at a JSONPath in a JSON body, as an assertion compares it: a string as the string,
/// any other value as JSON, several values as a JSON array of them. A path that finds nothing,
/// or a body that is not JSON, gives nothing, which no expected value equals.
fn json_value(body: &[u8], path: &str) -> String {
    let (Ok(path), Ok(document)) = (
        JsonPath::parse(path.trim()),
        serde_json::from_slice::<Value>(body),
    ) else {
        return String::new();
    };

    match path.query(&document).all().as_slice() {
        [] => String::new(),
        [Value::String(text)] => text.clone(),
        [value] => value.to_string(),
        values => Value::Array(values.iter().map(|value| (*value).clone()).collect()).to_string(),
    }
}

/// The value an extraction takes from the reply: what its pattern captures in the source, or the
/// whole of the source when it has no pattern (a JSONPath points at the value itself).
pub(super) fn extract(
    extraction: &wire::Extraction,
    observation: &HttpObservation,
) -> Result<Option<String>> {
    let source = source_value(&extraction.source, observation)
        .ok_or_else(|| anyhow!("unsupported extraction source `{}`", extraction.source))?;

    if extraction.expression.trim().is_empty() {
        return Ok((!source.is_empty()).then_some(source));
    }

    let regex = Regex::new(&extraction.expression)?;

    Ok(regex.captures(&source).and_then(|captures| {
        captures
            .get(1)
            .or_else(|| captures.get(0))
            .map(|value| value.as_str().to_string())
    }))
}

#[cfg(test)]
mod tests {
    use std::{
        net::{Ipv4Addr, SocketAddr},
        time::Duration,
    };

    use hyper::{StatusCode, Version, header::HeaderMap};

    use super::{HttpObservation, extract, source_value};
    use crate::protocol::v1 as wire;

    fn observation(body: &str) -> HttpObservation {
        HttpObservation {
            status: StatusCode::OK,
            version: Version::HTTP_2,
            headers: HeaderMap::new(),
            body: body.as_bytes().to_vec(),
            size: body.len(),
            elapsed: Duration::from_millis(12),
            remote: SocketAddr::from((Ipv4Addr::LOCALHOST, 443)),
            trace: None,
            before: [None, None, None, None],
            after: [
                Some(Duration::from_millis(3)),
                Some(Duration::from_millis(4)),
                None,
                Some(Duration::from_millis(5)),
            ],
        }
    }

    #[test]
    fn a_json_path_reads_one_value_or_several() {
        let reply = observation(r#"{"status":"up","checks":[{"ok":true},{"ok":false}],"count":2}"#);

        assert_eq!(source_value("json:$.status", &reply).as_deref(), Some("up"));
        assert_eq!(source_value("json:$.count", &reply).as_deref(), Some("2"));
        assert_eq!(
            source_value("json:$.checks[*].ok", &reply).as_deref(),
            Some("[true,false]")
        );
        assert_eq!(source_value("json:$.missing", &reply).as_deref(), Some(""));
        assert_eq!(
            source_value("json:$.status", &observation("not json")).as_deref(),
            Some("")
        );
    }

    #[test]
    fn size_and_phases_are_the_steps_own() {
        let reply = observation("ok");

        assert_eq!(source_value("size_bytes", &reply).as_deref(), Some("2"));
        assert_eq!(source_value("timing.dns_ms", &reply).as_deref(), Some("3"));
        assert_eq!(source_value("timing.tls_ms", &reply).as_deref(), Some("0"));
        assert_eq!(
            source_value("timing.first_byte_ms", &reply).as_deref(),
            Some("5")
        );
        assert_eq!(source_value("timing.ttl", &reply), None);
    }

    #[test]
    fn an_extraction_by_json_path_needs_no_pattern() {
        let reply = observation(r#"{"token":"abc123"}"#);

        let whole = wire::Extraction {
            variable: "token".into(),
            source: "json:$.token".into(),
            expression: String::new(),
            required: true,
        };

        let captured = wire::Extraction {
            expression: "abc(\\d+)".into(),
            ..whole.clone()
        };

        assert_eq!(extract(&whole, &reply).unwrap().as_deref(), Some("abc123"));
        assert_eq!(extract(&captured, &reply).unwrap().as_deref(), Some("123"));
    }
}
