// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The JSON output format of CloudWatch Metric Streams: one object per line.
//!
//! ```json
//! {"metric_stream_name":"MyMetricStream","account_id":"1234567890","region":"us-east-1",
//!  "namespace":"AWS/EC2","metric_name":"DiskWriteOps","dimensions":{"InstanceId":"i-123"},
//!  "timestamp":1611929698000,"value":{"count":3.0,"sum":20.0,"max":18.0,"min":0.0,"p99":17.56},
//!  "unit":"Seconds"}
//! ```
//!
//! See <https://docs.aws.amazon.com/AmazonCloudWatch/latest/monitoring/CloudWatch-metric-streams-formats-json.html>.

use domain::intake::RawEvent;
use kernel::time::TimestampMicros;
use serde_json::Value;

use super::{super::Delivery, Datum, statistic_suffix};

/// Whether `text` is a Metric Streams record: its first line is an object naming its stream.
pub fn is_metric_stream(text: &str) -> bool {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .and_then(|line| serde_json::from_str::<Value>(line).ok())
        .is_some_and(|first| {
            first.get("metric_stream_name").is_some() && first.get("value").is_some()
        })
}

/// The samples of every line, and how many lines could not be read.
pub fn samples(text: &str, delivery: &Delivery) -> (Vec<RawEvent>, usize) {
    let mut samples = Vec::new();
    let mut skipped = 0;

    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        match serde_json::from_str::<Value>(line)
            .ok()
            .and_then(|value| datum(&value))
        {
            Some(datum) => samples.extend(datum.samples(delivery)),
            None => skipped += 1,
        }
    }

    (samples, skipped)
}

fn datum(line: &Value) -> Option<Datum> {
    let text = |key: &str| line.get(key).and_then(Value::as_str).map(str::to_string);
    let namespace = text("namespace")?;
    let metric_name = text("metric_name")?;
    let value = line.get("value")?.as_object()?;
    let stat = |key: &str| value.get(key).and_then(Value::as_f64);

    let dimensions = line
        .get("dimensions")
        .and_then(Value::as_object)
        .map(|dimensions| {
            dimensions
                .iter()
                .filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let extra = value
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "count" | "sum" | "min" | "max"))
        .filter_map(|(key, value)| Some((statistic_suffix(key), value.as_f64()?)))
        .collect();

    Some(Datum {
        account_id: text("account_id"),
        region: text("region"),
        namespace,
        metric_name,
        dimensions,
        timestamp: line
            .get("timestamp")
            .and_then(Value::as_i64)
            .map(|ms| TimestampMicros(ms.saturating_mul(1_000)))
            .unwrap_or_else(TimestampMicros::now),
        unit: text("unit"),
        count: stat("count"),
        sum: stat("sum"),
        min: stat("min"),
        max: stat("max"),
        extra,
    })
}

#[cfg(test)]
mod tests {
    use domain::metrics::METRIC_NAME_FIELD;

    use super::{
        super::super::{FirehoseRequest, decode, tests::record},
        *,
    };

    const LINES: &str = concat!(
        r#"{"metric_stream_name":"s","account_id":"1234567890","region":"us-east-1","namespace":"AWS/EC2","metric_name":"DiskWriteOps","dimensions":{"InstanceId":"i-123"},"timestamp":1611929698000,"value":{"count":3.0,"sum":20.0,"max":18.0,"min":0.0,"p99":17.56,"TM(25%:75%)":16.43},"unit":"Seconds"}"#,
        "\n",
        r#"{"metric_stream_name":"s","account_id":"1234567890","region":"us-east-1","namespace":"AWS/EC2","metric_name":"CPUUtilization","dimensions":{},"timestamp":1611929698000,"value":{"count":1.0,"sum":7.5,"max":7.5,"min":7.5},"unit":"Percent"}"#,
        "\n",
        "not json\n",
    );

    #[test]
    fn a_metric_streams_record_becomes_metric_samples() {
        let request = FirehoseRequest {
            request_id: "r".into(),
            timestamp: 0,
            records: vec![record(LINES.as_bytes())],
        };

        let decoded = decode(request, &Delivery::default());
        let names: Vec<&str> = decoded
            .metrics
            .iter()
            .map(|sample| sample.fields[METRIC_NAME_FIELD].as_str().unwrap())
            .collect();

        assert!(decoded.logs.is_empty());
        assert_eq!(decoded.rejected, 1);
        assert_eq!(decoded.metrics.len(), 7 + 5);
        assert!(names.contains(&"aws_ec2_disk_write_ops_p99"));
        assert!(names.contains(&"aws_ec2_disk_write_ops_tm_25_75"));
        assert!(names.contains(&"aws_ec2_cpuutilization_average"));

        let first = &decoded.metrics[0];

        assert_eq!(first.timestamp.0, 1_611_929_698_000_000);
        assert_eq!(first.fields["dimension_InstanceId"], "i-123");
        assert_eq!(first.fields["account_id"], "1234567890");
    }

    #[test]
    fn ordinary_json_lines_are_not_metric_streams() {
        assert!(!is_metric_stream("{\"msg\":\"hi\"}\n"));
        assert!(is_metric_stream(LINES));
    }
}
