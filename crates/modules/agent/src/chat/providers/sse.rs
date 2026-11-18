// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Shared SSE parsing utilities for Mole Agent Model Gateway.
//!
//! Feeds `reqwest::Response::bytes_stream()` into [`eventsource_stream::EventStream`],
//! producing `SseEvent` (with an `event` name + `data` field). Each adapter decides how to decode
//! JSON.
//!
//! `eventsource_stream` already handles:
//! - Single-line / multiline `data:` concatenation (joined with `\n`)
//! - `event:` name parsing (defaults to "message")
//! - Ignoring `retry:` / comment lines
//! - SSE frame boundaries (blank lines)

use std::pin::Pin;

use bytes::Bytes;
use eventsource_stream::{Event, Eventsource};
use futures::stream::{Stream, StreamExt};
use kernel::Error;

pub type SseEvent = Event;

/// Converts a byte stream into an SSE frame stream; converts errors to `kernel::Error`.
pub fn parse_sse<S>(byte_stream: S) -> impl Stream<Item = Result<SseEvent, Error>>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    byte_stream
        .map(|chunk| chunk.map_err(|e| Error::internal(format!("sse stream: {e}"))))
        .eventsource()
        .map(|r| r.map_err(|e| Error::internal(format!("sse parse: {e}"))))
}

/// Converts a reqwest streaming response into an `SseEvent` stream with a unified type signature.
pub fn response_to_sse(
    resp: reqwest::Response,
) -> Pin<Box<dyn Stream<Item = Result<SseEvent, Error>> + Send>> {
    Box::pin(parse_sse(resp.bytes_stream()))
}

#[cfg(test)]
mod tests {
    use futures::stream;

    use super::*;

    fn synth(bytes: &[u8]) -> impl Stream<Item = Result<Bytes, reqwest::Error>> {
        stream::iter(vec![Ok::<Bytes, reqwest::Error>(Bytes::copy_from_slice(
            bytes,
        ))])
    }

    #[tokio::test]
    async fn parses_single_data_frame() {
        let bytes = b"data: {\"hello\":\"world\"}\n\n";
        let mut s = parse_sse(synth(bytes));
        let evt = s.next().await.unwrap().unwrap();
        assert_eq!(evt.event, "message");
        assert_eq!(evt.data, "{\"hello\":\"world\"}");
    }

    #[tokio::test]
    async fn parses_named_event_with_data() {
        let bytes = b"event: chunk\ndata: hello\n\n";
        let mut s = parse_sse(synth(bytes));
        let evt = s.next().await.unwrap().unwrap();
        assert_eq!(evt.event, "chunk");
        assert_eq!(evt.data, "hello");
    }

    #[tokio::test]
    async fn handles_multiline_data() {
        let bytes = b"data: line1\ndata: line2\n\n";
        let mut s = parse_sse(synth(bytes));
        let evt = s.next().await.unwrap().unwrap();
        assert_eq!(evt.data, "line1\nline2");
    }
}
