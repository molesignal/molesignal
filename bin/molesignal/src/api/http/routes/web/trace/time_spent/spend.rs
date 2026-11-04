// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How a sample of requests spent their time, from their Spans: each part
//! exclusive of what runs under it, the request's own time what none of its
//! Spans covers.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use serde_json::Value;

/// How deep under a request its Spans are followed.
const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in super::super) enum TimeCategory {
    /// The request's own time: none of its Spans ran then.
    Own,
    Database,
    Cache,
    Http,
    Rpc,
    Messaging,
    /// Internal Spans of the service under the request.
    Internal,
    /// Calls out that say nothing of what they call.
    Other,
}

#[derive(Debug, PartialEq, Serialize)]
pub(in super::super) struct TimePart {
    pub(super) category: TimeCategory,
    pub(super) micros: u64,
}

/// A sampled request: its trace, its Span, how long it took in microseconds.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Request {
    pub(super) trace: String,
    pub(super) span: String,
    pub(super) micros: u64,
}

/// A Span of a sampled trace, as far as where it sits and what it calls.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SpanRow {
    pub(super) trace: String,
    pub(super) span: String,
    pub(super) parent: String,
    pub(super) kind: SpanKind,
    pub(super) micros: u64,
    pub(super) calls: Option<TimeCategory>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum SpanKind {
    Internal,
    Server,
    Client,
    Producer,
    Consumer,
    Unspecified,
}

/// A Span's kind as the stream keeps it: the number OTLP gives it, or text.
pub(in super::super) fn kind_of(value: Option<&Value>) -> SpanKind {
    let text = match value {
        Some(Value::Number(number)) => number.to_string(),
        Some(Value::String(text)) => text.to_ascii_uppercase(),
        _ => return SpanKind::Unspecified,
    };

    match text.trim_start_matches("SPAN_KIND_") {
        "1" | "INTERNAL" => SpanKind::Internal,
        "2" | "SERVER" => SpanKind::Server,
        "3" | "CLIENT" => SpanKind::Client,
        "4" | "PRODUCER" => SpanKind::Producer,
        "5" | "CONSUMER" => SpanKind::Consumer,
        _ => SpanKind::Unspecified,
    }
}

/// What a call out calls, as APM tells its dependencies apart; `None` for a
/// Span that calls nothing out.
pub(super) fn classify(kind: SpanKind, said: &[(&str, String)]) -> Option<TimeCategory> {
    if !matches!(kind, SpanKind::Client | SpanKind::Producer) {
        return None;
    }

    let has = |field: &str| said.iter().any(|(name, _)| *name == field);
    let system = said
        .iter()
        .find(|(name, _)| matches!(*name, "db.system.name" | "db.system"))
        .map(|(_, value)| value.to_ascii_lowercase());

    Some(match system {
        Some(system) if matches!(system.as_str(), "redis" | "memcached" | "valkey") => {
            TimeCategory::Cache
        }
        Some(_) => TimeCategory::Database,
        None if has("messaging.system") || kind == SpanKind::Producer => TimeCategory::Messaging,
        None if has("rpc.service") || has("rpc.system") => TimeCategory::Rpc,
        None if has("http.request.method")
            || has("http.method")
            || has("server.address")
            || has("peer.service") =>
        {
            TimeCategory::Http
        }
        None => TimeCategory::Other,
    })
}

/// The requests' time together and where it went, the largest part first.
pub(super) fn spend(requests: &[Request], spans: &[SpanRow]) -> (u64, Vec<TimePart>) {
    let mut children = HashMap::<(&str, &str), Vec<&SpanRow>>::new();

    for span in spans {
        children
            .entry((span.trace.as_str(), span.parent.as_str()))
            .or_default()
            .push(span);
    }

    let mut parts = BTreeMap::<TimeCategory, u64>::new();
    let mut total = 0_u64;

    for request in requests {
        total = total.saturating_add(request.micros);
        let under = walk(&children, &request.trace, &request.span, 0, &mut parts);
        *parts.entry(TimeCategory::Own).or_default() += request.micros.saturating_sub(under);
    }

    // Calls made side by side add up past the requests' time: scale them to it.
    let counted = parts.values().copied().sum::<u64>();
    let scale = if counted > total && counted > 0 {
        total as f64 / counted as f64
    } else {
        1.0
    };

    let mut parts = parts
        .into_iter()
        .map(|(category, micros)| TimePart {
            category,
            micros: (micros as f64 * scale).round() as u64,
        })
        .filter(|part| part.micros > 0)
        .collect::<Vec<_>>();

    parts.sort_by(|left, right| {
        right
            .micros
            .cmp(&left.micros)
            .then(left.category.cmp(&right.category))
    });
    (total, parts)
}

/// Adds the time of the Spans under `span` to `parts`, each exclusive of
/// those under it; the time of the Spans right under it, together.
fn walk(
    children: &HashMap<(&str, &str), Vec<&SpanRow>>,
    trace: &str,
    span: &str,
    depth: usize,
    parts: &mut BTreeMap<TimeCategory, u64>,
) -> u64 {
    let Some(under) = children.get(&(trace, span)).filter(|_| depth < MAX_DEPTH) else {
        return 0;
    };
    let mut time = 0_u64;

    for child in under {
        time = time.saturating_add(child.micros);

        let category = match child.calls {
            Some(category) => category,
            // Another service's request is its own: it is not followed.
            None if matches!(child.kind, SpanKind::Server | SpanKind::Consumer) => {
                TimeCategory::Other
            }
            None => {
                let nested = walk(children, trace, &child.span, depth + 1, parts);
                *parts.entry(TimeCategory::Internal).or_default() +=
                    child.micros.saturating_sub(nested);
                continue;
            }
        };

        *parts.entry(category).or_default() += child.micros;
    }

    time
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(
        id: &str,
        parent: &str,
        kind: SpanKind,
        micros: u64,
        calls: Option<TimeCategory>,
    ) -> SpanRow {
        SpanRow {
            trace: "t1".into(),
            span: id.into(),
            parent: parent.into(),
            kind,
            micros,
            calls,
        }
    }

    #[test]
    fn a_request_is_split_into_its_calls_its_internal_work_and_its_own_time() {
        let requests = [Request {
            trace: "t1".into(),
            span: "entry".into(),
            micros: 100,
        }];
        let spans = [
            span(
                "db",
                "entry",
                SpanKind::Client,
                30,
                Some(TimeCategory::Database),
            ),
            span("work", "entry", SpanKind::Internal, 40, None),
            span(
                "cache",
                "work",
                SpanKind::Client,
                10,
                Some(TimeCategory::Cache),
            ),
            // Another trace's Span with the same parent id is not this request's.
            SpanRow {
                trace: "t2".into(),
                ..span("x", "entry", SpanKind::Client, 50, Some(TimeCategory::Http))
            },
        ];

        let (total, parts) = spend(&requests, &spans);

        assert_eq!(total, 100);
        assert_eq!(
            parts,
            // Equal parts in the order the categories are listed in.
            vec![
                TimePart {
                    category: TimeCategory::Own,
                    micros: 30
                },
                TimePart {
                    category: TimeCategory::Database,
                    micros: 30
                },
                TimePart {
                    category: TimeCategory::Internal,
                    micros: 30
                },
                TimePart {
                    category: TimeCategory::Cache,
                    micros: 10
                },
            ]
        );
    }

    #[test]
    fn calls_made_side_by_side_are_scaled_to_the_request() {
        let requests = [Request {
            trace: "t1".into(),
            span: "entry".into(),
            micros: 100,
        }];
        let spans = [
            span(
                "a",
                "entry",
                SpanKind::Client,
                100,
                Some(TimeCategory::Http),
            ),
            span("b", "entry", SpanKind::Client, 100, Some(TimeCategory::Rpc)),
        ];

        let (_, parts) = spend(&requests, &spans);

        assert_eq!(parts.iter().map(|part| part.micros).sum::<u64>(), 100);
        assert!(parts.iter().all(|part| part.category != TimeCategory::Own));
    }

    #[test]
    fn a_call_is_told_by_what_it_says() {
        let said = |pairs: &[(&'static str, &str)]| {
            pairs
                .iter()
                .map(|(name, value)| (*name, (*value).to_owned()))
                .collect::<Vec<_>>()
        };

        assert_eq!(
            classify(SpanKind::Client, &said(&[("db.system", "redis")])),
            Some(TimeCategory::Cache)
        );
        assert_eq!(
            classify(SpanKind::Client, &said(&[("db.system.name", "postgresql")])),
            Some(TimeCategory::Database)
        );
        assert_eq!(
            classify(SpanKind::Producer, &said(&[])),
            Some(TimeCategory::Messaging)
        );
        assert_eq!(
            classify(SpanKind::Client, &said(&[("rpc.service", "Payments")])),
            Some(TimeCategory::Rpc)
        );
        assert_eq!(
            classify(SpanKind::Client, &said(&[("http.request.method", "GET")])),
            Some(TimeCategory::Http)
        );
        assert_eq!(
            classify(SpanKind::Client, &said(&[])),
            Some(TimeCategory::Other)
        );
        assert_eq!(
            classify(SpanKind::Internal, &said(&[("db.system", "redis")])),
            None
        );
        assert_eq!(kind_of(Some(&Value::from(3))), SpanKind::Client);
        assert_eq!(
            kind_of(Some(&Value::from("SPAN_KIND_PRODUCER"))),
            SpanKind::Producer
        );
    }
}
