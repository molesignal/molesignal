// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Mole Agent model telemetry fan-out hook.
//!
//! Integration point: after the OTLP traces receiver converts an OTLP span -> RawEvent, call
//! [`AgentFanoutHook::extract`] to extract 0..n Agent-derived events; returns empty for non-LLM
//! spans. Derived events follow the same `IntakeService::intake` path as the original trace into a
//! second stream (`stream_type = Traces, stream = "agent_model_traces"`), making them accessible
//! through regular queries, the query cache, and the Tantivy index.

use domain::intake::RawEvent;
use serde_json::{Map, Value};

use super::redact::redact_pii;

/// Agent-derived event extractor.
/// Stateless, attached to the traces intake path.
#[derive(Default, Clone)]
pub struct AgentFanoutHook;

impl AgentFanoutHook {
    pub fn new() -> Self {
        Self
    }

    /// Takes a trace event and returns a possible LLM-derived event.
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

        // External applications report costs in dollars (`gen_ai.usage.cost_usd`), recorded in the
        // same two columns as Mole Agent's own calls.
        if let Some(cost) = ev.fields.get("gen_ai.usage.cost_usd") {
            out.insert("cost".to_string(), cost.clone());
            out.insert("cost_currency".to_string(), Value::String("USD".into()));
        }
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

pub fn extract_batch(hook: &AgentFanoutHook, events: &[RawEvent]) -> Vec<RawEvent> {
    events.iter().filter_map(|e| hook.extract(e)).collect()
}

#[cfg(test)]
mod tests {
    use kernel::time::TimestampMicros;

    use super::*;

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
        let hook = AgentFanoutHook::new();
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
        let hook = AgentFanoutHook::new();
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
