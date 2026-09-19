// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{
    sync::{Arc, mpsc},
    time::Duration,
};

use anyhow::{Context as _, Result, anyhow, bail};
use base64::Engine as _;
use headless_chrome::{
    Tab,
    protocol::cdp::{IO, Tracing, types::Event},
};
use serde_json::Value as JsonValue;

use super::super::ExecutionContext;

pub(super) const MAX_TRACE_BYTES: usize = 5 * 1024 * 1024;
const TRACE_BUFFER_KIB: f64 = 4096.0;

pub(super) struct TraceCapture {
    completion: mpsc::Receiver<Tracing::events::TracingCompleteEventParams>,
    active: bool,
}

impl TraceCapture {
    pub(super) fn start(tab: &Tab) -> Result<Self> {
        let (sender, completion) = mpsc::sync_channel(1);
        tab.add_event_listener(Arc::new(move |event: &Event| {
            if let Event::TracingComplete(event) = event {
                let _ = sender.try_send(event.params.clone());
            }
        }))?;
        tab.call_method(Tracing::Start {
            categories: None,
            options: None,
            buffer_usage_reporting_interval: None,
            transfer_mode: Some(Tracing::StartTransfer_modeOption::ReturnAsStream),
            stream_format: Some(Tracing::StreamFormat::Json),
            stream_compression: Some(Tracing::StreamCompression::None),
            trace_config: Some(Tracing::TraceConfig {
                record_mode: Some(Tracing::TraceConfigRecordMode::RecordUntilFull),
                trace_buffer_size_in_kb: Some(TRACE_BUFFER_KIB),
                enable_sampling: Some(true),
                enable_systrace: Some(false),
                enable_argument_filter: Some(true),
                included_categories: Some(vec![
                    "devtools.timeline".into(),
                    "loading".into(),
                    "navigation".into(),
                    "blink.user_timing".into(),
                ]),
                excluded_categories: None,
                synthetic_delays: None,
                memory_dump_config: None,
            }),
            perfetto_config: None,
            tracing_backend: Some(Tracing::TracingBackend::Chrome),
        })?;
        Ok(Self {
            completion,
            active: true,
        })
    }

    pub(super) fn finish(
        &mut self,
        tab: &Tab,
        context: &ExecutionContext,
        capture: bool,
        timeout: Duration,
    ) -> Result<Option<Vec<u8>>> {
        if !self.active {
            return Ok(None);
        }
        self.active = false;
        tab.call_method(Tracing::End(None))?;
        let completed = self
            .completion
            .recv_timeout(timeout)
            .context("wait for Chrome trace completion")?;
        let handle = completed
            .stream
            .ok_or_else(|| anyhow!("Chrome trace did not return a stream"))?;
        if !capture {
            let _ = tab.call_method(IO::Close {
                handle: handle.clone(),
            });
            return Ok(None);
        }

        let read = read_trace(tab, &handle);
        let _ = tab.call_method(IO::Close { handle });
        let bytes = read?;
        let mut value: JsonValue =
            serde_json::from_slice(&bytes).context("decode Chrome trace JSON")?;
        redact_json(&mut value, context);
        let bytes = serde_json::to_vec(&value).context("encode redacted Chrome trace")?;
        if bytes.len() > MAX_TRACE_BYTES {
            bail!("Browser trace exceeds the 5 MiB limit");
        }
        Ok(Some(bytes))
    }
}

fn read_trace(tab: &Tab, handle: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        let chunk = tab.call_method(IO::Read {
            handle: handle.to_string(),
            offset: None,
            size: Some(64 * 1024),
        })?;
        let data = if chunk.base_64_encoded.unwrap_or(false) {
            base64::engine::general_purpose::STANDARD
                .decode(chunk.data)
                .context("decode Chrome trace chunk")?
        } else {
            chunk.data.into_bytes()
        };
        if bytes.len().saturating_add(data.len()) > MAX_TRACE_BYTES {
            bail!("Browser trace exceeds the 5 MiB limit");
        }
        bytes.extend_from_slice(&data);
        if chunk.eof {
            return Ok(bytes);
        }
    }
}

pub(super) fn redact_json(value: &mut JsonValue, context: &ExecutionContext) {
    match value {
        JsonValue::String(value) => *value = context.redact(value),
        JsonValue::Array(values) => {
            for value in values {
                redact_json(value, context);
            }
        }
        JsonValue::Object(values) => {
            let original = std::mem::take(values);
            for (key, mut value) in original {
                redact_json(&mut value, context);
                values.insert(context.redact(&key), value);
            }
        }
        JsonValue::Null | JsonValue::Bool(_) | JsonValue::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::redact_json;
    use crate::executor::ExecutionContext;

    #[test]
    fn trace_json_redacts_nested_secrets() {
        let context = ExecutionContext {
            secrets: HashMap::from([("token".into(), b"secret/value".to_vec())]),
            variables: HashMap::new(),
        };
        let mut value = json!({
            "secret/value": { "url": "https://example.test/?token=secret%2Fvalue" }
        });
        redact_json(&mut value, &context);
        assert_eq!(
            value["[REDACTED]"]["url"],
            "https://example.test/?token=[REDACTED]"
        );
    }
}
