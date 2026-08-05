// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence Model Gateway 共享 SSE 解析工具。
//!
//! 把 `reqwest::Response::bytes_stream()` 喂给 [`eventsource_stream::EventStream`]，
//! 输出 `SseEvent`（含 `event` 名 + `data` 字段）。Adapter 自行决定 JSON 怎么解。
//!
//! `eventsource_stream` 已处理：
//! - `data:` 单行 / 多行拼接（用 `\n` join）
//! - `event:` 名解析（默认 "message"）
//! - `retry:` / 注释行忽略
//! - SSE 帧边界（空行）

use std::pin::Pin;

use bytes::Bytes;
use eventsource_stream::{Event, Eventsource};
use futures::stream::{Stream, StreamExt};

use crate::shared::Error;

pub type SseEvent = Event;

/// 把字节流转 SSE 帧流；错误转 `crate::shared::Error`。
pub fn parse_sse<S>(byte_stream: S) -> impl Stream<Item = Result<SseEvent, Error>>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    byte_stream
        .map(|chunk| chunk.map_err(|e| Error::internal(format!("sse stream: {e}"))))
        .eventsource()
        .map(|r| r.map_err(|e| Error::internal(format!("sse parse: {e}"))))
}

/// 把 reqwest streaming response 转 `SseEvent` 流，统一类型签名。
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
