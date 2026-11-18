// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Which fields of a stream hold what an investigation needs: the service, the environment, the
//! trace id, the message, a time written as text.
//!
//! A field is found by one of the names it is known by, in their order: named so exactly, or
//! else named so once case and separators are set aside (`service.name`, `service_name` and
//! `serviceName` are one name). System fields, which begin with `_`, are never one of them.

use domain::stream::{FieldDef, FieldType, Schema};

/// The names the service is recorded under, the conventional ones first.
pub(super) const SERVICE_FIELDS: &[&str] = &[
    "service.name",
    "service_name",
    "service",
    "app",
    "app_name",
    "application",
    "application_name",
];

/// The same for metrics, where the `job` of a scrape names the service when nothing else does.
pub(super) const METRIC_SERVICE_FIELDS: &[&str] = &[
    "service.name",
    "service_name",
    "service",
    "app",
    "app_name",
    "application",
    "application_name",
    "job",
];

/// The service fields every signal can share: OpenTelemetry's name and its flat form.
pub(super) const CONVENTIONAL_SERVICE_FIELDS: &[&str] = &["service.name", "service_name"];

pub(super) const ENVIRONMENT_FIELDS: &[&str] = &[
    "deployment.environment.name",
    "deployment.environment",
    "deployment_environment",
    "environment",
    "env",
];

pub(super) const TRACE_ID_FIELDS: &[&str] = &["trace_id", "otel.trace_id", "dd.trace_id"];

/// The fields a log says its message in, as the Logs page looks for them.
pub(super) const MESSAGE_FIELDS: &[&str] = &[
    "message", "body", "summary", "log", "event", "error", "msg", "content", "text",
];

/// Fields that hold the time of the event as text, beside the system time.
pub(super) const TEXT_TIME_FIELDS: &[&str] = &[
    "time",
    "timestamp",
    "@timestamp",
    "datetime",
    "event_time",
    "log_time",
];

/// What a stream's fields hold.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Detected {
    pub(super) service: Option<String>,
    pub(super) environment: Option<String>,
    pub(super) trace_id: Option<String>,
    pub(super) message: Option<String>,
    /// The message field holds text, so it can be looked into.
    pub(super) message_is_text: bool,
    pub(super) text_time: Option<String>,
    /// How many fields the stream has besides the system ones.
    pub(super) own_fields: usize,
}

/// Letters and digits alone, lower case.
pub(super) fn plain(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect()
}

fn is_system(field: &FieldDef) -> bool {
    field.name.starts_with('_')
}

/// The stream's field known by one of `names`: an exact name first, in their order, then a name
/// that reads the same in letters and digits.
pub(super) fn find<'a>(schema: &'a Schema, names: &[&str]) -> Option<&'a FieldDef> {
    let own = || schema.fields.iter().filter(|field| !is_system(field));

    names
        .iter()
        .find_map(|name| own().find(|field| field.name == *name))
        .or_else(|| {
            names.iter().find_map(|name| {
                let wanted = plain(name);
                own().find(|field| plain(&field.name) == wanted)
            })
        })
}

/// What the fields of a stream hold. `metrics` reads the scrape `job` as the service too.
pub(super) fn detect(schema: &Schema, metrics: bool) -> Detected {
    let service_names = if metrics {
        METRIC_SERVICE_FIELDS
    } else {
        SERVICE_FIELDS
    };

    let message = find(schema, MESSAGE_FIELDS);

    let text_time = TEXT_TIME_FIELDS.iter().find_map(|name| {
        schema.fields.iter().find(|field| {
            !is_system(field)
                && field.data_type == FieldType::Utf8
                && plain(&field.name) == plain(name)
        })
    });

    Detected {
        service: find(schema, service_names).map(|field| field.name.clone()),
        environment: find(schema, ENVIRONMENT_FIELDS).map(|field| field.name.clone()),
        trace_id: find(schema, TRACE_ID_FIELDS).map(|field| field.name.clone()),
        message: message.map(|field| field.name.clone()),
        message_is_text: message.is_some_and(|field| field.data_type == FieldType::Utf8),
        text_time: text_time.map(|field| field.name.clone()),
        own_fields: schema
            .fields
            .iter()
            .filter(|field| !is_system(field))
            .count(),
    }
}

/// Whether the service is recorded under a name every signal can share.
pub(super) fn is_conventional_service_field(name: &str) -> bool {
    CONVENTIONAL_SERVICE_FIELDS.contains(&name)
}

#[cfg(test)]
mod tests {
    use domain::stream::StreamIndexType;

    use super::*;

    fn schema(fields: &[(&str, FieldType)]) -> Schema {
        Schema {
            fields: fields
                .iter()
                .map(|(name, data_type)| FieldDef {
                    name: (*name).to_string(),
                    data_type: *data_type,
                    nullable: true,
                    index_type: StreamIndexType::None,
                    encrypted: false,
                })
                .collect(),
        }
    }

    #[test]
    fn the_conventional_name_wins_over_a_synonym() {
        let found = detect(
            &schema(&[
                ("_timestamp", FieldType::Timestamp),
                ("app", FieldType::Utf8),
                ("service_name", FieldType::Utf8),
                ("env", FieldType::Utf8),
                ("trace_id", FieldType::Utf8),
                ("message", FieldType::Utf8),
            ]),
            false,
        );

        assert_eq!(found.service.as_deref(), Some("service_name"));
        assert_eq!(found.environment.as_deref(), Some("env"));
        assert_eq!(found.trace_id.as_deref(), Some("trace_id"));
        assert_eq!(found.message.as_deref(), Some("message"));
        assert!(found.message_is_text);
        assert_eq!(found.own_fields, 5);
    }

    #[test]
    fn a_name_written_another_way_is_found_by_its_letters() {
        let found = detect(
            &schema(&[
                ("ServiceName", FieldType::Utf8),
                ("traceId", FieldType::Utf8),
                ("Deployment.Environment", FieldType::Utf8),
            ]),
            false,
        );

        assert_eq!(found.service.as_deref(), Some("ServiceName"));
        assert_eq!(found.trace_id.as_deref(), Some("traceId"));
        assert_eq!(found.environment.as_deref(), Some("Deployment.Environment"));
        assert_eq!(found.message, None);
    }

    #[test]
    fn a_synonym_is_found_and_job_counts_only_for_metrics() {
        let fields = schema(&[("job", FieldType::Utf8), ("app_name", FieldType::Utf8)]);

        assert_eq!(detect(&fields, false).service.as_deref(), Some("app_name"));

        let scrape = schema(&[("job", FieldType::Utf8), ("instance", FieldType::Utf8)]);

        assert_eq!(detect(&scrape, true).service.as_deref(), Some("job"));
        assert_eq!(detect(&scrape, false).service, None);
    }

    #[test]
    fn the_system_time_is_not_a_text_time_but_a_text_field_named_time_is() {
        let found = detect(
            &schema(&[
                ("_timestamp", FieldType::Timestamp),
                ("timestamp", FieldType::Utf8),
                ("log", FieldType::Json),
            ]),
            false,
        );

        assert_eq!(found.text_time.as_deref(), Some("timestamp"));
        assert_eq!(found.message.as_deref(), Some("log"));
        assert!(!found.message_is_text);
        assert!(is_conventional_service_field("service.name"));
        assert!(!is_conventional_service_field("app"));
    }
}
