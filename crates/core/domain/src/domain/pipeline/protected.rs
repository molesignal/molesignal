// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 实时流水线不许改的字段，按它处理的数据声明（[`Guard`]）。
//!
//! - 指标：标签以外的一切（`metric_name`、取值、时间、元数据、exemplar 列）；OTLP 直方图和摘要
//!   存成普通列的 `count`、`sum`、`min`、`max`；再加上直方图的 `le` 和摘要的 `quantile`：它们说的
//!   是这个样本是哪个桶、哪个分位，改了 `histogram_quantile` 就算错。改别的标签会改变序列身份，
//!   这正是降基数要的。
//! - 链路：一条 trace 的骨架和时间：`trace_id`、`span_id`、`parent_span_id`、`service.name`、
//!   起止时间和耗时。
//! - RUM：只有自定义部分能改（[`RUM_CUSTOM_FIELDS`]），其余是 RUM 页面读的契约列。页面和 Web
//!   Vitals 一行都不能丢，会话由页面合成，没有流水线处理它。
//! - 性能剖析：流水线只处理元数据行，对象本身不动。对象位置（`object_key`、`archived_bytes`）、ID、
//!   时间和测出来的量都守住（[`PROTECTED_PROFILE_FIELDS`]）；服务名和 `labels` 能改，也能添字段，也能
//!   丢掉一行（对象留到保留期过了再清）。
//! - 别的日志不守字段。
//!
//! 运行时在每一步之后比对（[`changed_protected`]）：改了、删了、或者添了一个受保护的字段，这条
//! 事件就算处理失败。

use serde_json::{Map, Value};

use crate::domain::{metrics::is_metric_label_name, rum::event::RumStream, stream::StreamType};

/// 一个 span 的骨架和时间：trace 树、服务和时长都靠它们。
pub const PROTECTED_SPAN_FIELDS: &[&str] = &[
    "trace_id",
    "span_id",
    "parent_span_id",
    "service.name",
    "start_time_unix_nano",
    "end_time_unix_nano",
    "duration_ns",
];

/// 说明样本是哪个桶、哪个分位的标签。
pub const PROTECTED_METRIC_LABELS: &[&str] = &["le", "quantile"];

/// OTLP 直方图和摘要的样本存成普通列的量：和取值一样是测出来的数，不是标签。恰好叫这些名字的标签
/// 也一并守住。
pub const PROTECTED_METRIC_VALUES: &[&str] = &["count", "sum", "min", "max"];

/// RUM 行里应用自己的部分：自定义上下文、用户的自定义属性、特性开关，以及没有单独列的字段。
pub const RUM_CUSTOM_FIELDS: &[&str] = &["context", "usr.extra", "feature_flags", "extra"];

/// 一条性能剖析元数据行里不许改的：它指向哪个对象、是什么类型、什么时候、测出了多少。火焰图和下载
/// 靠它们找到并合并对象。
pub const PROTECTED_PROFILE_FIELDS: &[&str] = &[
    "id",
    "object_key",
    "archived_bytes",
    "_timestamp",
    "profile_type",
    "duration_nanos",
    "sample_count",
    "total_value",
    "unsymbolized",
    "trace_id",
    "span_id",
];

/// 一条实时流水线处理的数据守住什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guard {
    Metrics,
    Spans,
    /// 性能剖析的元数据行。
    Profiles,
    /// 一个 RUM 流。
    Rum(RumStream),
}

impl Guard {
    /// 流 `stream`（数据种类 `signal`）上的实时流水线守住什么；不守字段的日志流是 `None`。
    pub fn of(signal: StreamType, stream: &str) -> Option<Self> {
        if signal == StreamType::METRICS {
            return Some(Self::Metrics);
        }

        if signal == StreamType::TRACES {
            return Some(Self::Spans);
        }

        if signal == StreamType::PROFILES {
            return Some(Self::Profiles);
        }

        RumStream::from_name(stream).map(Self::Rum)
    }

    /// `field` 是不是步骤不许改的字段。
    pub fn keeps(self, field: &str) -> bool {
        match self {
            Self::Metrics => {
                !is_metric_label_name(field)
                    || PROTECTED_METRIC_LABELS.contains(&field)
                    || PROTECTED_METRIC_VALUES.contains(&field)
            }
            Self::Spans => PROTECTED_SPAN_FIELDS.contains(&field),
            Self::Profiles => PROTECTED_PROFILE_FIELDS.contains(&field),
            Self::Rum(_) => !RUM_CUSTOM_FIELDS.contains(&field),
        }
    }

    /// 步骤能不能丢掉一条事件：RUM 的页面和 Web Vitals 一行都不能丢。
    pub fn drops(self) -> bool {
        !matches!(
            self,
            Self::Rum(RumStream::Views | RumStream::Vitals | RumStream::Sessions)
        )
    }

    /// 消息里怎么称呼这份数据。
    pub fn name(self) -> &'static str {
        match self {
            Self::Metrics => "metrics",
            Self::Spans => "traces",
            Self::Profiles => "profiles",
            Self::Rum(stream) => stream.name(),
        }
    }
}

/// 事件里受保护的字段，留着在每一步之后比对。
pub fn protected_of(guard: Guard, fields: &Map<String, Value>) -> Map<String, Value> {
    fields
        .iter()
        .filter(|(name, _)| guard.keeps(name))
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect()
}

/// 步骤之后的事件里第一个被改、被删或被添上的受保护字段；都没动时是 `None`。不是对象的事件
/// 不在这里判断。
pub fn changed_protected(
    guard: Guard,
    before: &Map<String, Value>,
    after: &Value,
) -> Option<String> {
    let after = after.as_object()?;

    if let Some((name, _)) = before
        .iter()
        .find(|(name, value)| after.get(name.as_str()) != Some(value))
    {
        return Some(name.clone());
    }

    after
        .keys()
        .find(|name| !before.contains_key(name.as_str()) && guard.keeps(name))
        .cloned()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn fields(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn a_metric_keeps_everything_but_its_labels() {
        let sample = fields(json!({
            "metric_name": "http_requests_total",
            "value": 3,
            "metric_kind": "counter",
            "__molesignal_exemplar": "1",
            "le": "0.5",
            "pod": "web-7f9c",
            "env": "prod",
        }));
        let before = protected_of(Guard::Metrics, &sample);
        let mut kept: Vec<&str> = before.keys().map(String::as_str).collect();
        kept.sort_unstable();

        assert_eq!(
            kept,
            vec![
                "__molesignal_exemplar",
                "le",
                "metric_kind",
                "metric_name",
                "value"
            ]
        );

        let mut after = Value::Object(sample.clone());
        after.as_object_mut().unwrap().remove("pod");
        after["cluster"] = json!("east");
        assert_eq!(changed_protected(Guard::Metrics, &before, &after), None);

        for (change, field) in [
            (json!({ "metric_name": "requests" }), "metric_name"),
            (json!({ "value": 4 }), "value"),
            (json!({ "le": "+Inf" }), "le"),
            (json!({ "metric_unit": "s" }), "metric_unit"),
            (json!({ "sum": 210.5 }), "sum"),
        ] {
            let mut after = Value::Object(sample.clone());

            for (name, value) in change.as_object().unwrap() {
                after[name] = value.clone();
            }

            assert_eq!(
                changed_protected(Guard::Metrics, &before, &after).as_deref(),
                Some(field)
            );
        }

        let mut renamed = Value::Object(sample);
        let name = renamed
            .as_object_mut()
            .unwrap()
            .remove("metric_name")
            .unwrap();
        renamed["name"] = name;
        assert_eq!(
            changed_protected(Guard::Metrics, &before, &renamed).as_deref(),
            Some("metric_name")
        );
    }

    #[test]
    fn a_span_keeps_its_place_in_the_trace_and_its_timing() {
        let span = fields(json!({
            "trace_id": "t1",
            "span_id": "s2",
            "parent_span_id": "s1",
            "service.name": "checkout",
            "name": "GET /users/42",
            "duration_ns": 4000,
            "db.statement": "SELECT * FROM users WHERE id = 42",
        }));
        let before = protected_of(Guard::Spans, &span);

        let mut after = Value::Object(span.clone());
        after["name"] = json!("GET /users/{id}");
        after.as_object_mut().unwrap().remove("db.statement");
        assert_eq!(changed_protected(Guard::Spans, &before, &after), None);

        let mut detached = Value::Object(span);
        detached.as_object_mut().unwrap().remove("parent_span_id");
        assert_eq!(
            changed_protected(Guard::Spans, &before, &detached).as_deref(),
            Some("parent_span_id")
        );
    }

    #[test]
    fn a_rum_row_changes_only_in_its_custom_parts() {
        let view = fields(json!({
            "view.url": "https://shop.example/cart?id=42",
            "usr.id": "u-1",
            "context": { "plan": "pro" },
            "usr.extra": {},
        }));
        let guard = Guard::of(StreamType::LOGS, "rum_views").unwrap();
        let before = protected_of(guard, &view);

        let mut after = Value::Object(view.clone());
        after["context"]["team"] = json!("checkout");
        after["usr.extra"] = json!({ "tier": "gold" });
        after["feature_flags"] = json!({ "new_cart": true });
        assert_eq!(changed_protected(guard, &before, &after), None);

        let mut moved = Value::Object(view);
        moved["view.url"] = json!("https://shop.example/cart");
        assert_eq!(
            changed_protected(guard, &before, &moved).as_deref(),
            Some("view.url")
        );

        // Views and vitals are never dropped; actions, errors, resources and long tasks may be.
        assert!(!guard.drops());
        assert!(!Guard::Rum(RumStream::Vitals).drops());
        assert!(Guard::of(StreamType::LOGS, "rum_actions").unwrap().drops());
        assert!(Guard::Metrics.drops());
    }

    #[test]
    fn a_profile_row_keeps_where_its_object_is_and_when_it_was_taken() {
        let row = fields(json!({
            "id": "p-1",
            "service": "checkout",
            "profile_type": "cpu",
            "object_key": "blobs/v1/o/profiles/checkout/cpu/20261031/p-1.pprof.zst",
            "archived_bytes": 4096,
            "duration_nanos": 10_000_000_000_i64,
            "sample_count": 12,
            "total_value": 150,
            "unsymbolized": false,
            "trace_id": "",
            "span_id": "",
            "labels": { "pod": "web-1" },
        }));
        let guard = Guard::of(StreamType::PROFILES, "default").unwrap();
        let before = protected_of(guard, &row);

        assert!(guard.drops());

        // A label added, a label taken away, the service renamed: fine.
        let mut after = Value::Object(row.clone());
        after["labels"] = json!({ "env": "prod" });
        after["service"] = json!("payments");
        after["team"] = json!("core");
        assert_eq!(changed_protected(guard, &before, &after), None);

        for (field, value) in [
            ("object_key", json!("elsewhere")),
            ("id", json!("p-2")),
            ("profile_type", json!("memory")),
            ("total_value", json!(0)),
            ("_timestamp", json!(1)),
        ] {
            let mut changed = Value::Object(row.clone());
            changed[field] = value;

            assert_eq!(
                changed_protected(guard, &before, &changed).as_deref(),
                Some(field)
            );
        }
    }

    #[test]
    fn other_logs_keep_nothing() {
        assert_eq!(Guard::of(StreamType::LOGS, "app_logs"), None);
        assert_eq!(Guard::of(StreamType::METRICS, "cpu"), Some(Guard::Metrics));
        assert_eq!(Guard::of(StreamType::TRACES, "default"), Some(Guard::Spans));
    }
}
