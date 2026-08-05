// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Datadog-RUM 兼容 JSON 解析。
//!
//! 3 个常规事件 endpoint：
//! - POST /api/v1/rum/sessions  → stream `rum_sessions`
//! - POST /api/v1/rum/actions   → stream `rum_actions`
//! - POST /api/v1/rum/errors    → stream `rum_errors`
//!
//! Replay 由 `infra::rum::replay` 独立验证并写入 object store 与元数据表。
//!
//! 同一 payload 可以是单条 object 或 NDJSON / Array；本层把入参一律展平为 Vec<RawEvent>。

use serde_json::{Map, Value};

use crate::{
    domain::ingestion::RawEvent,
    shared::{Error, Result, time::TimestampMicros},
};

/// 把任意 JSON 形态（单对象 / 数组 / NDJSON）转成 RawEvent 序列。
pub fn flatten(payload: Value) -> Result<Vec<RawEvent>> {
    match payload {
        Value::Object(map) => Ok(vec![into_event(map)?]),
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                match it {
                    Value::Object(m) => out.push(into_event(m)?),
                    other => {
                        return Err(Error::invalid(format!(
                            "rum payload item must be object, got {other}"
                        )));
                    }
                }
            }
            Ok(out)
        }
        _ => Err(Error::invalid("rum payload must be object or array")),
    }
}

/// NDJSON：按行解析。
pub fn flatten_ndjson(body: &str) -> Result<Vec<RawEvent>> {
    let mut out = Vec::new();
    for line in body.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let v: Value =
            serde_json::from_str(l).map_err(|e| Error::invalid(format!("rum ndjson: {e}")))?;
        match v {
            Value::Object(m) => out.push(into_event(m)?),
            _ => return Err(Error::invalid("rum ndjson line must be object")),
        }
    }
    Ok(out)
}

fn into_event(mut m: Map<String, Value>) -> Result<RawEvent> {
    let ts = extract_timestamp(&mut m);
    Ok(RawEvent {
        timestamp: ts,
        fields: m,
    })
}

fn extract_timestamp(m: &mut Map<String, Value>) -> TimestampMicros {
    // 优先 `_timestamp`（项目惯例），否则 `timestamp`，否则 `date`，否则 now。
    for key in ["_timestamp", "timestamp", "date"] {
        if let Some(v) = m.remove(key)
            && let Some(n) = v.as_i64()
        {
            // 若值看起来是毫秒（< 10^14）则升到 us；否则按 us。
            let us = if n.unsigned_abs() < 100_000_000_000_000_u64 {
                n.saturating_mul(1_000)
            } else {
                n
            };
            m.insert(key.to_string(), Value::from(us));
            return TimestampMicros(us);
        }
    }
    TimestampMicros::now()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn flatten_object() {
        let v = json!({"action": "click", "_timestamp": 100_000_000_i64});
        let out = flatten(v).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].timestamp.0, 100_000_000_000); // ms → us
    }

    #[test]
    fn flatten_array() {
        let v = json!([{"a": 1}, {"a": 2}]);
        let out = flatten(v).unwrap();
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn flatten_ndjson_skips_blank() {
        let body = "{\"a\":1}\n\n{\"a\":2}\n";
        let out = flatten_ndjson(body).unwrap();
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn rejects_non_object() {
        let v = json!("nope");
        assert!(flatten(v).is_err());
    }
}
