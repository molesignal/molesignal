// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Amazon Data Firehose HTTP endpoint delivery: what a request carries and what its records hold.
//!
//! A request is `{requestId, timestamp, records: [{data: base64}]}` (see
//! <https://docs.aws.amazon.com/firehose/latest/dev/httpdeliveryrequestresponse.html>). Firehose
//! adds no delimiter between records, so each record is read on its own, by what it holds:
//!
//! - gzip bytes are unpacked first: a CloudWatch Logs subscription hands Firehose each batch
//!   gzipped, and Firehose passes it on as it is;
//! - a CloudWatch Logs subscription message becomes one log event per entry of `logEvents`
//!   ([`cloudwatch_logs`]);
//! - a CloudWatch Metric Streams record, in JSON or OpenTelemetry, becomes metric samples
//!   ([`metric_streams`]);
//! - any other text is a log event per line, the fields of a JSON object or the line as `message`.
//!
//! A record that cannot be read is counted and left out; the rest of the request is still taken,
//! since Firehose retries a refused request whole and would only refuse it again.

use std::io::Read as _;

use base64::Engine as _;
use domain::intake::RawEvent;
use flate2::read::GzDecoder;
use kernel::time::TimestampMicros;
use serde::Deserialize;
use serde_json::{Map, Value};

pub mod cloudwatch_logs;
pub mod metric_streams;

/// The largest request body Firehose sends: its buffer goes up to 64 MiB, counted before
/// compression.
pub const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

/// What one record may unpack to. A record is at most 1,000 KiB before base64, and CloudWatch Logs
/// compresses its batches at gzip level 6.
const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;

/// What the records of one request may unpack to together.
const MAX_UNPACKED_BYTES: usize = 256 * 1024 * 1024;

/// The gzip magic number.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirehoseRequest {
    /// Echoed back in the answer; Firehose keeps it across retries of the same delivery.
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub timestamp: i64,
    pub records: Vec<FirehoseRecord>,
}

#[derive(Debug, Deserialize)]
pub struct FirehoseRecord {
    pub data: String,
}

/// What the request headers say about the delivery as a whole.
#[derive(Debug, Default, Clone)]
pub struct Delivery {
    /// The account the Firehose stream belongs to, from `X-Amz-Firehose-Source-Arn`.
    pub account_id: Option<String>,
    /// Its region, from the same header: a CloudWatch Logs message does not carry one.
    pub region: Option<String>,
    /// `X-Amz-Firehose-Common-Attributes`: set on the stream, added to every event.
    pub common: Map<String, Value>,
}

impl Delivery {
    /// Reads the `X-Amz-Firehose-Source-Arn` and `X-Amz-Firehose-Common-Attributes` header values.
    /// One that cannot be read is ignored: it only adds fields.
    pub fn from_headers(source_arn: Option<&str>, common_attributes: Option<&str>) -> Self {
        // arn:aws:firehose:<region>:<account>:deliverystream/<name>
        let mut parts = source_arn.unwrap_or_default().split(':');
        let region = parts.nth(3).filter(|s| !s.is_empty()).map(str::to_string);
        let account_id = parts.next().filter(|s| !s.is_empty()).map(str::to_string);

        let common = common_attributes
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .and_then(|value| match value.get("commonAttributes") {
                Some(Value::Object(fields)) => Some(fields.clone()),
                _ => None,
            })
            .unwrap_or_default();

        Self {
            account_id,
            region,
            common,
        }
    }

    /// Adds the common attributes to `fields`, leaving any field the record already has.
    pub fn stamp(&self, fields: &mut Map<String, Value>) {
        for (key, value) in &self.common {
            fields.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
}

/// What the records of one request turned into.
#[derive(Debug, Default)]
pub struct Decoded {
    pub logs: Vec<RawEvent>,
    pub metrics: Vec<RawEvent>,
    /// Records that could not be read at all.
    pub rejected: usize,
}

/// Reads every record of `request`.
pub fn decode(request: FirehoseRequest, delivery: &Delivery) -> Decoded {
    let mut out = Decoded::default();
    let mut budget = MAX_UNPACKED_BYTES;

    for record in request.records {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(record.data.as_bytes())
        else {
            out.rejected += 1;
            continue;
        };

        let bytes = if bytes.starts_with(&GZIP_MAGIC) {
            match gunzip(&bytes, MAX_RECORD_BYTES.min(budget)) {
                Some(unpacked) => {
                    budget = budget.saturating_sub(unpacked.len());
                    unpacked
                }
                None => {
                    out.rejected += 1;
                    continue;
                }
            }
        } else {
            bytes
        };

        if !decode_record(&bytes, delivery, &mut out) {
            out.rejected += 1;
        }
    }

    out
}

/// Reads one unpacked record into `out`; false when it is none of the shapes Firehose brings.
fn decode_record(bytes: &[u8], delivery: &Delivery, out: &mut Decoded) -> bool {
    if bytes.is_empty() {
        return true;
    }

    if let Some(message) = cloudwatch_logs::message(bytes) {
        out.logs.extend(cloudwatch_logs::events(&message, delivery));
        return true;
    }

    if is_text(bytes) {
        let text = String::from_utf8_lossy(bytes);

        if metric_streams::json::is_metric_stream(&text) {
            let (samples, skipped) = metric_streams::json::samples(&text, delivery);
            out.metrics.extend(samples);
            out.rejected += skipped;
        } else {
            out.logs.extend(lines(&text, delivery));
        }

        return true;
    }

    match metric_streams::otel::samples(bytes, delivery) {
        Some(samples) => {
            out.metrics.extend(samples);
            true
        }
        None => false,
    }
}

/// Whether a record reads as text: UTF-8 without control characters other than whitespace near
/// its start. Protobuf frames start with a length and a field tag, which are not.
fn is_text(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(256)];

    std::str::from_utf8(bytes).is_ok()
        && head
            .iter()
            .all(|byte| !byte.is_ascii_control() || matches!(byte, b'\n' | b'\r' | b'\t'))
}

/// Unpacks a gzip record, or `None` when it is broken or larger than `limit` once unpacked.
fn gunzip(bytes: &[u8], limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();

    GzDecoder::new(bytes)
        .take(limit as u64 + 1)
        .read_to_end(&mut out)
        .ok()?;

    (out.len() <= limit).then_some(out)
}

/// A log event per non-empty line: a JSON object gives the fields, anything else is `message`.
fn lines(text: &str, delivery: &Delivery) -> Vec<RawEvent> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut fields = match serde_json::from_str::<Value>(line) {
                Ok(Value::Object(fields)) => fields,
                Ok(other) => Map::from_iter([("payload".to_string(), other)]),
                Err(_) => Map::from_iter([("message".to_string(), Value::String(line.into()))]),
            };

            delivery.stamp(&mut fields);

            RawEvent {
                timestamp: TimestampMicros::now(),
                fields,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use flate2::{Compression, write::GzEncoder};

    use super::*;

    pub(super) fn record(bytes: &[u8]) -> FirehoseRecord {
        FirehoseRecord {
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }

    pub(super) fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    fn request(records: Vec<FirehoseRecord>) -> FirehoseRequest {
        FirehoseRequest {
            request_id: "r".into(),
            timestamp: 0,
            records,
        }
    }

    #[test]
    fn json_and_text_lines_become_log_events() {
        let decoded = decode(
            request(vec![record(
                b"{\"level\":\"info\",\"msg\":\"hi\"}\nplain text line\n",
            )]),
            &Delivery::default(),
        );

        assert_eq!(decoded.logs.len(), 2);
        assert!(decoded.logs[0].fields.contains_key("level"));
        assert_eq!(decoded.logs[1].fields["message"], "plain text line");
        assert_eq!(decoded.rejected, 0);
    }

    #[test]
    fn a_gzipped_record_is_unpacked_before_it_is_read() {
        let decoded = decode(
            request(vec![record(&gzip(b"{\"msg\":\"packed\"}"))]),
            &Delivery::default(),
        );

        assert_eq!(decoded.logs.len(), 1);
        assert_eq!(decoded.logs[0].fields["msg"], "packed");
    }

    #[test]
    fn records_that_cannot_be_read_are_counted_and_the_rest_kept() {
        let decoded = decode(
            request(vec![
                FirehoseRecord {
                    data: "not base64!".into(),
                },
                record(&[0x1f, 0x8b, 0x00]),
                record(&[0x0a, 0xff, 0x01, 0x02]),
                record(b"{\"ok\":true}"),
            ]),
            &Delivery::default(),
        );

        assert_eq!(decoded.rejected, 3);
        assert_eq!(decoded.logs.len(), 1);
    }

    #[test]
    fn the_headers_give_the_account_region_and_common_attributes() {
        let delivery = Delivery::from_headers(
            Some("arn:aws:firehose:eu-west-1:123456789012:deliverystream/logs"),
            Some(r#"{"commonAttributes":{"team":"payments","msg":"never wins"}}"#),
        );

        assert_eq!(delivery.account_id.as_deref(), Some("123456789012"));
        assert_eq!(delivery.region.as_deref(), Some("eu-west-1"));

        let decoded = decode(request(vec![record(b"{\"msg\":\"hi\"}")]), &delivery);

        assert_eq!(decoded.logs[0].fields["team"], "payments");
        assert_eq!(decoded.logs[0].fields["msg"], "hi");
    }

    #[test]
    fn headers_that_cannot_be_read_add_nothing() {
        let delivery = Delivery::from_headers(Some("not an arn"), Some("{not json"));

        assert!(delivery.account_id.is_none());
        assert!(delivery.region.is_none());
        assert!(delivery.common.is_empty());
    }

    #[test]
    fn a_record_larger_than_the_limit_once_unpacked_is_refused() {
        assert!(gunzip(&gzip(&vec![b'a'; 1024]), 1023).is_none());
        assert_eq!(gunzip(&gzip(&vec![b'a'; 1024]), 1024).unwrap().len(), 1024);
    }
}
