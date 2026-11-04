// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Writing a run's events into the pipeline's extend tables (`sink_tables`): a row per event under
//! the value of the table's key field, which VRL's `lookup(table, key)` then finds. A table with
//! value fields takes those fields of the event; one without takes the whole event. A key that is
//! already there is replaced, so the latest event wins.
//!
//! An event without the key field is not written to that table: it counts as failed and becomes a
//! dead letter (stage `write`, destination `table:<name>`). So do the events for a table that does
//! not exist. A write that fails (storage unavailable) leaves the window to be tried again for the
//! sources whose events were not written; rows are written by key, so writing them again is
//! harmless.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::{
    DeadLetter, Ledger, PipelineOutput, PipelineSpec, WriteFailure, is_configuration_error,
};
use crate::{
    infra::pipeline::{
        ExtendRow, ExtendTableSummary, ExtendValueField, exec::processing::ProcessedEvent,
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// The key under which a source's rows count as written to an extend table.
pub(super) fn table_key(table: &str) -> String {
    format!("table:{table}")
}

pub(super) async fn write(
    output: &PipelineOutput,
    pipeline: &PipelineSpec<'_>,
    rows: &[ProcessedEvent],
    ledger: &mut Ledger,
) {
    if pipeline.config.tables.is_empty() {
        return;
    }

    let definitions = match &output.tables {
        Some(store) => store.list_tables(pipeline.org_id).await,
        None => Err(crate::shared::Error::unavailable(
            "extend tables cannot be written on this node",
        )),
    };

    let definitions = match definitions {
        Ok(definitions) => definitions,
        Err(e) => {
            let failure = WriteFailure {
                message: format!("read the extend tables: {e}"),
                permanent: is_configuration_error(&e),
            };

            ledger.errors.push(failure.message.clone());

            for (index, row) in rows.iter().enumerate() {
                ledger.failed[index] = true;
                ledger.sources[row.source]
                    .failure
                    .get_or_insert_with(|| failure.clone());
            }

            return;
        }
    };

    for table in &pipeline.config.tables {
        let destination = table_key(table);

        let indices: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, row)| !ledger.sources[row.source].done.contains(&destination))
            .map(|(index, _)| index)
            .collect();

        let failed_sources = match definitions.iter().find(|known| known.table_name == *table) {
            None => {
                let reason = format!("extend table `{table}` does not exist");
                unwritten(rows, &indices, ledger, &destination, &reason);
                BTreeSet::new()
            }
            Some(definition) => {
                write_table(output, pipeline, rows, &indices, ledger, definition).await
            }
        };

        for (index, source) in ledger.sources.iter_mut().enumerate() {
            if !failed_sources.contains(&index) {
                source.done.insert(destination.clone());
            }
        }
    }
}

/// Writes the rows of one table. Returns the sources whose rows a failed write left unwritten.
async fn write_table(
    output: &PipelineOutput,
    pipeline: &PipelineSpec<'_>,
    rows: &[ProcessedEvent],
    indices: &[usize],
    ledger: &mut Ledger,
    definition: &ExtendTableSummary,
) -> BTreeSet<usize> {
    let table = definition.table_name.as_str();
    let key_field = definition.key_field.as_str();
    let destination = table_key(table);
    let now = TimestampMicros::now();
    let mut written = Vec::new();
    let mut keyless = Vec::new();

    let mut table_rows = Vec::new();

    // A run reads its rows in no order of time, and of the rows of one key the last one stays:
    // going through them oldest first makes that the latest event.
    let mut oldest_first = indices.to_vec();
    oldest_first.sort_by_key(|&index| event_time(&rows[index].event));

    for &index in &oldest_first {
        let event = &rows[index].event;

        let Some(key) = key_of(event, key_field) else {
            keyless.push(index);
            continue;
        };

        table_rows.push(ExtendRow {
            id: Id::new(),
            org_id: pipeline.org_id.clone(),
            table_name: table.to_owned(),
            key,
            value_json: value_of(event, &definition.value_fields),
            updated_at: now,
        });
        written.push(index);
    }

    if !keyless.is_empty() {
        let reason = format!("no `{key_field}` to key a row of extend table `{table}`");
        unwritten(rows, &keyless, ledger, &destination, &reason);
    }

    let Some(store) = &output.tables else {
        return BTreeSet::new();
    };

    if table_rows.is_empty() {
        return BTreeSet::new();
    }

    let Err(e) = store.upsert_rows(table_rows).await else {
        return BTreeSet::new();
    };

    let failure = WriteFailure {
        message: format!("write to extend table `{table}`: {e}"),
        permanent: is_configuration_error(&e),
    };

    ledger.errors.push(failure.message.clone());

    let mut failed_sources = BTreeSet::new();

    for index in written {
        let source = rows[index].source;
        ledger.failed[index] = true;
        failed_sources.insert(source);
        ledger.sources[source]
            .failure
            .get_or_insert_with(|| failure.clone());
    }

    failed_sources
}

/// Counts `indices` as failed for one table: their events become dead letters with the reason.
fn unwritten(
    rows: &[ProcessedEvent],
    indices: &[usize],
    ledger: &mut Ledger,
    destination: &str,
    reason: &str,
) {
    if indices.is_empty() {
        return;
    }

    ledger.errors.push(format!(
        "{} event(s) were not written: {reason}",
        indices.len()
    ));

    for &index in indices {
        let row = &rows[index];
        let source = ledger.names[row.source].clone();
        ledger.failed[index] = true;
        ledger.dead_letter(|| DeadLetter::rejected(source, row.event.clone(), destination, reason));
    }
}

/// When an event happened, read the way a row written to a stream takes it (`to_raw_event`).
fn event_time(event: &Value) -> Option<i64> {
    event
        .get("_timestamp")
        .or_else(|| event.get("timestamp"))
        .and_then(Value::as_i64)
}

/// The text of the key field: a literal key first, then a dotted path. Text is taken as it is,
/// numbers and booleans as written; anything else, or nothing, has no key.
fn key_of(event: &Value, field: &str) -> Option<String> {
    let value = event.get(field).or_else(|| {
        field
            .split('.')
            .try_fold(event, |value, key| value.get(key))
    })?;

    match value {
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

/// The row of an event: the table's value fields it has, or the whole event when the table names
/// none.
fn value_of(event: &Value, value_fields: &[ExtendValueField]) -> Value {
    if value_fields.is_empty() {
        return event.clone();
    }

    let fields: Map<String, Value> = value_fields
        .iter()
        .filter_map(|field| {
            event
                .get(&field.name)
                .filter(|value| !value.is_null())
                .map(|value| (field.name.clone(), value.clone()))
        })
        .collect();

    Value::Object(fields)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn field(name: &str) -> ExtendValueField {
        ExtendValueField {
            name: name.into(),
            field_type: "string".into(),
            required: false,
            description: String::new(),
        }
    }

    #[test]
    fn a_row_is_keyed_by_text_numbers_or_a_dotted_path() {
        assert_eq!(
            key_of(&json!({ "host": "web-01" }), "host"),
            Some("web-01".into())
        );
        assert_eq!(key_of(&json!({ "id": 42 }), "id"), Some("42".into()));
        assert_eq!(
            key_of(&json!({ "k8s": { "pod": "p1" } }), "k8s.pod"),
            Some("p1".into())
        );
        assert_eq!(
            key_of(
                &json!({ "k8s.pod": "literal", "k8s": { "pod": "p1" } }),
                "k8s.pod"
            ),
            Some("literal".into())
        );

        for missing in [
            json!({}),
            json!({ "host": "" }),
            json!({ "host": null }),
            json!({ "host": [1] }),
        ] {
            assert_eq!(key_of(&missing, "host"), None);
        }
    }

    #[test]
    fn a_row_holds_the_table_s_value_fields_or_the_whole_event() {
        let event = json!({ "host": "web-01", "owner": "alice", "team": null, "noise": 1 });

        assert_eq!(value_of(&event, &[]), event);
        assert_eq!(
            value_of(&event, &[field("owner"), field("team"), field("site")]),
            json!({ "owner": "alice" })
        );
    }
}
