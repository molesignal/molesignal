// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::LazyLock;

use super::{
    action::ACTION,
    common::COMMON,
    error::ERROR,
    field::RumField,
    resource::{LOG, LONG_TASK, RESOURCE, VITAL},
    session::SESSION,
    stream::RumStream,
    view::VIEW,
};
use crate::domain::{
    intake::EVENT_ID_FIELD,
    stream::{FieldDef, FieldType, Schema, StreamIndexType},
};

static TABLES: LazyLock<[Vec<RumField>; 8]> = LazyLock::new(|| RumStream::ALL.map(build));

fn build(stream: RumStream) -> Vec<RumField> {
    let specific = match stream {
        RumStream::Sessions => return SESSION.to_vec(),
        RumStream::Views => VIEW,
        RumStream::Actions => ACTION,
        RumStream::Errors => ERROR,
        RumStream::Resources => RESOURCE,
        RumStream::LongTasks => LONG_TASK,
        RumStream::Vitals => VITAL,
        RumStream::Logs => LOG,
    };

    COMMON.iter().chain(specific).copied().collect()
}

/// The columns of a stream, in the order its schema lists them.
pub fn fields(stream: RumStream) -> &'static [RumField] {
    &TABLES[stream as usize]
}

pub fn field(stream: RumStream, name: &str) -> Option<&'static RumField> {
    fields(stream).iter().find(|field| field.name == name)
}

/// The fixed schema a RUM stream is created with. Types are declared, never inferred: the first
/// `view.performance.cls.score` an SDK sends may well be `0`.
pub fn stream_schema(stream: RumStream) -> Schema {
    let mut fields = vec![FieldDef {
        name: EVENT_ID_FIELD.to_string(),
        data_type: FieldType::Utf8,
        nullable: true,
        index_type: Some(StreamIndexType::Exact),
        indexed: true,
        encrypted: false,
        exact: true,
    }];

    fields.extend(self::fields(stream).iter().map(RumField::field_def));

    Schema { fields }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn column_names_are_unique_within_a_stream() {
        for stream in RumStream::ALL {
            let mut seen = HashSet::new();

            for field in fields(stream) {
                assert!(
                    seen.insert(field.name),
                    "{} twice in {stream:?}",
                    field.name
                );
            }
        }
    }

    #[test]
    fn every_event_stream_collects_what_it_does_not_know() {
        for stream in RumStream::ALL {
            let has_extra = field(stream, "extra").is_some();

            assert_eq!(has_extra, stream != RumStream::Sessions, "{stream:?}");
        }

        for stream in RumStream::ALL {
            if stream != RumStream::Sessions {
                assert!(field(stream, "usr.extra").is_some(), "{stream:?}");
            }
        }
    }

    #[test]
    fn versioned_streams_keep_their_key_and_revision_as_columns() {
        for stream in RumStream::ALL {
            let Some(versioned) = stream.versioned() else {
                continue;
            };

            assert!(field(stream, versioned.key).is_some(), "{stream:?}");
            assert_eq!(
                field(stream, versioned.revision).map(|field| field.data_type),
                Some(FieldType::Int64)
            );
        }
    }

    #[test]
    fn streams_that_are_not_versioned_declare_no_revision() {
        for stream in RumStream::ALL {
            if stream.versioned().is_some() {
                continue;
            }

            assert!(
                fields(stream)
                    .iter()
                    .all(|field| !field.name.ends_with(".revision")),
                "{stream:?}"
            );
        }
    }

    #[test]
    fn schemas_start_with_the_event_id_and_declare_every_column() {
        for stream in RumStream::ALL {
            let schema = stream_schema(stream);

            assert_eq!(schema.fields[0].name, EVENT_ID_FIELD);
            assert_eq!(schema.fields.len(), fields(stream).len() + 1);
            assert!(schema.fields.iter().all(|field| field.nullable));
        }
    }

    #[test]
    fn stable_dimensions_are_indexed_exactly() {
        for (stream, name) in [
            (RumStream::Views, "view.id"),
            (RumStream::Resources, "session.id"),
            (RumStream::Errors, "error.fingerprint"),
            (RumStream::Errors, "error.issue_id"),
            (RumStream::Resources, "resource.trace_id"),
            (RumStream::Sessions, "usr.id"),
        ] {
            let field = field(stream, name).unwrap();

            assert_eq!(field.index, StreamIndexType::Exact, "{name}");
        }
    }
}
