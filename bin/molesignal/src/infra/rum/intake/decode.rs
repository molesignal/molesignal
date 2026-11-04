// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The body of `POST /api/v1/rum/events`: optionally gzip-compressed, then a JSON array of events,
//! one event, or one event per line.

use std::io::Read;

use flate2::read::GzDecoder;
use serde_json::Value;

use crate::shared::{Error, Result};

/// Most events one request may carry.
pub const MAX_EVENTS_PER_REQUEST: usize = 5000;

/// Most bytes a request may hold once decompressed.
pub const MAX_DECODED_BYTES: usize = 5 * 1024 * 1024;

/// An event as the body had it, or why one line of an NDJSON body could not be read.
pub type Decoded = std::result::Result<Value, String>;

/// What a request holds.
pub struct DecodedBody {
    pub events: Vec<Decoded>,
    /// The bytes of the body once decompressed.
    pub bytes: usize,
}

/// Turns the body of a request into its events. A body that is not JSON at all fails the whole
/// request; a bad line in an NDJSON body fails only that line.
pub fn decode_body(
    body: &[u8],
    content_encoding: Option<&str>,
    content_type: Option<&str>,
) -> Result<DecodedBody> {
    let inflated;

    let body = match content_encoding.map(|value| value.trim().to_ascii_lowercase()) {
        None => body,
        Some(encoding) if encoding.is_empty() || encoding == "identity" => body,
        Some(encoding) if encoding == "gzip" => {
            inflated = inflate(body)?;
            inflated.as_slice()
        }
        Some(encoding) => {
            return Err(Error::invalid(format!(
                "unsupported Content-Encoding `{encoding}`; use gzip or none"
            )));
        }
    };

    if body.len() > MAX_DECODED_BYTES {
        return Err(too_large());
    }

    let events = if is_ndjson(content_type) {
        lines(body)
    } else {
        whole(body)?
    };

    if events.len() > MAX_EVENTS_PER_REQUEST {
        return Err(Error::payload_too_large(format!(
            "a request may carry at most {MAX_EVENTS_PER_REQUEST} events"
        )));
    }

    Ok(DecodedBody {
        events,
        bytes: body.len(),
    })
}

fn inflate(body: &[u8]) -> Result<Vec<u8>> {
    let mut decoded = Vec::new();

    // One byte more than the limit tells a body that fits from one that does not.
    GzDecoder::new(body)
        .take(MAX_DECODED_BYTES as u64 + 1)
        .read_to_end(&mut decoded)
        .map_err(|error| Error::invalid(format!("gzip body cannot be read: {error}")))?;

    if decoded.len() > MAX_DECODED_BYTES {
        return Err(too_large());
    }

    Ok(decoded)
}

fn too_large() -> Error {
    Error::payload_too_large(format!(
        "a request may hold at most {MAX_DECODED_BYTES} bytes once decompressed"
    ))
}

fn is_ndjson(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|value| {
        let value = value.to_ascii_lowercase();

        value.contains("ndjson") || value.contains("jsonl")
    })
}

fn lines(body: &[u8]) -> Vec<Decoded> {
    body.split(|byte| *byte == b'\n')
        .filter(|line| !line.iter().all(u8::is_ascii_whitespace))
        .map(|line| serde_json::from_slice::<Value>(line).map_err(|error| error.to_string()))
        .collect()
}

fn whole(body: &[u8]) -> Result<Vec<Decoded>> {
    match serde_json::from_slice::<Value>(body)
        .map_err(|error| Error::invalid(format!("body is not JSON: {error}")))?
    {
        Value::Array(items) => Ok(items.into_iter().map(Ok).collect()),
        event @ Value::Object(_) => Ok(vec![Ok(event)]),
        _ => Err(Error::invalid(
            "body must be a JSON array of events or one event",
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};
    use serde_json::json;

    use super::*;

    fn decode(
        body: &[u8],
        content_encoding: Option<&str>,
        content_type: Option<&str>,
    ) -> Result<Vec<Decoded>> {
        decode_body(body, content_encoding, content_type).map(|decoded| decoded.events)
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());

        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn an_array_gives_its_events() {
        let events = decode(br#"[{"type":"view"},{"type":"action"}]"#, None, None).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].as_ref().unwrap(), &json!({"type": "view"}));
    }

    #[test]
    fn one_object_is_one_event() {
        let events = decode(br#"{"type":"view"}"#, None, Some("application/json")).unwrap();

        assert_eq!(events.len(), 1);
    }

    #[test]
    fn a_bad_ndjson_line_fails_only_that_line() {
        let body = b"{\"type\":\"view\"}\n\n not json \n{\"type\":\"action\"}\n";
        let events = decode(body, None, Some("application/x-ndjson")).unwrap();

        assert_eq!(events.len(), 3);
        assert!(events[0].is_ok());
        assert!(events[1].is_err());
        assert!(events[2].is_ok());
    }

    #[test]
    fn a_body_that_is_not_json_fails_the_request() {
        assert!(decode(b"nope", None, None).is_err());
        assert!(decode(b"42", None, None).is_err());
        assert!(decode(b"\"text\"", None, None).is_err());
    }

    #[test]
    fn gzip_is_inflated() {
        let body = gzip(br#"[{"type":"view"}]"#);
        let events = decode(&body, Some("gzip"), None).unwrap();

        assert_eq!(events.len(), 1);
        assert!(decode(&body, Some("GZIP "), None).is_ok());
        assert!(decode(b"{}", Some("identity"), None).is_ok());
    }

    #[test]
    fn other_encodings_and_broken_gzip_are_refused() {
        assert!(decode(b"{}", Some("br"), None).is_err());
        assert!(decode(b"not gzip", Some("gzip"), None).is_err());
    }

    #[test]
    fn a_body_that_inflates_past_the_limit_is_refused() {
        let events = format!("[{}]", "0,".repeat(MAX_DECODED_BYTES / 2 + 16));
        let body = gzip(events.as_bytes());

        assert!(body.len() < MAX_DECODED_BYTES);

        let error = decode(&body, Some("gzip"), None).unwrap_err();

        assert!(matches!(error, Error::PayloadTooLarge(_)));
    }

    #[test]
    fn too_many_events_are_refused() {
        let body = format!("[{}]", vec!["{}"; MAX_EVENTS_PER_REQUEST + 1].join(","));
        let error = decode(body.as_bytes(), None, None).unwrap_err();

        assert!(matches!(error, Error::PayloadTooLarge(_)));
    }
}
