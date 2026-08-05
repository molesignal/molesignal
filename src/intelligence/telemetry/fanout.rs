// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Intelligence 模型遥测 fan-out hook。
//!
//! 集成位：在 OTLP traces receiver 把 OTLP span → RawEvent 之后，调
//! [`IntelligenceFanoutHook::extract`] 抽出 0..n 条 Intelligence 派生事件；非 LLM span 返空。
//! 派生事件走与原 trace 同样的 `IngestService::ingest` 路径写入第二份
//! stream（`stream_type = Traces, stream = "intelligence_model_traces"`），从而透过普通查询/查询
//! 缓存/Tantivy 索引接通。

use serde_json::{Map, Value};

use super::redact::redact_pii;
use crate::domain::ingestion::RawEvent;

/// Intelligence 派生事件抽取器。无状态，挂在 traces ingest path。
#[derive(Default, Clone)]
pub struct IntelligenceFanoutHook;

impl IntelligenceFanoutHook {
    pub fn new() -> Self {
        Self
    }

    /// 输入一条 trace 事件，输出可能的 LLM 派生事件。
    pub fn extract(&self, ev: &RawEvent) -> Option<RawEvent> {
        let has_gen_ai = ev.fields.keys().any(|k| k.starts_with("gen_ai."));
        if !has_gen_ai {
            return None;
        }
        let mut out: Map<String, Value> = Map::new();
        let copy = |dst: &mut Map<String, Value>, src: &Map<String, Value>, k: &str, dk: &str| {
            if let Some(v) = src.get(k) {
                dst.insert(dk.to_string(), v.clone());
            }
        };
        copy(&mut out, &ev.fields, "gen_ai.system", "provider");
        copy(&mut out, &ev.fields, "gen_ai.request.model", "model");
        copy(
            &mut out,
            &ev.fields,
            "gen_ai.usage.prompt_tokens",
            "prompt_tokens",
        );
        copy(
            &mut out,
            &ev.fields,
            "gen_ai.usage.completion_tokens",
            "completion_tokens",
        );
        copy(
            &mut out,
            &ev.fields,
            "gen_ai.usage.total_tokens",
            "total_tokens",
        );
        copy(&mut out, &ev.fields, "gen_ai.usage.cost_usd", "cost_usd");
        copy(&mut out, &ev.fields, "duration_ms", "latency_ms");
        copy(&mut out, &ev.fields, "status", "status");
        copy(&mut out, &ev.fields, "error", "error");

        if let Some(Value::String(p)) = ev.fields.get("gen_ai.prompt") {
            out.insert("prompt_redacted".to_string(), Value::String(redact_pii(p)));
        }
        if let Some(Value::String(c)) = ev.fields.get("gen_ai.completion") {
            out.insert(
                "completion_redacted".to_string(),
                Value::String(redact_pii(c)),
            );
        }

        Some(RawEvent {
            timestamp: ev.timestamp,
            fields: out,
        })
    }
}

pub fn extract_batch(hook: &IntelligenceFanoutHook, events: &[RawEvent]) -> Vec<RawEvent> {
    events.iter().filter_map(|e| hook.extract(e)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::time::TimestampMicros;

    fn ev(map: Map<String, Value>) -> RawEvent {
        RawEvent {
            timestamp: TimestampMicros(0),
            fields: map,
        }
    }

    #[test]
    fn extract_skips_non_llm_spans() {
        let mut m = Map::new();
        m.insert("http.method".to_string(), Value::String("GET".into()));
        let hook = IntelligenceFanoutHook::new();
        assert!(hook.extract(&ev(m)).is_none());
    }

    #[test]
    fn extract_maps_gen_ai_attributes_and_redacts() {
        let mut m = Map::new();
        m.insert("gen_ai.system".to_string(), Value::String("openai".into()));
        m.insert(
            "gen_ai.request.model".to_string(),
            Value::String("gpt-4o".into()),
        );
        m.insert(
            "gen_ai.usage.total_tokens".to_string(),
            Value::from(150_i64),
        );
        m.insert(
            "gen_ai.prompt".to_string(),
            Value::String("email foo@bar.com please".into()),
        );
        let hook = IntelligenceFanoutHook::new();
        let derived = hook.extract(&ev(m)).unwrap();
        assert_eq!(
            derived.fields.get("provider"),
            Some(&Value::String("openai".into()))
        );
        assert_eq!(
            derived.fields.get("model"),
            Some(&Value::String("gpt-4o".into()))
        );
        let p = derived
            .fields
            .get("prompt_redacted")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(p.contains("<email>"));
    }
}
