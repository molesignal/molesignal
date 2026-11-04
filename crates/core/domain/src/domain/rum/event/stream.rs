// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

/// The kinds of event an SDK sends. A session is not one of them: the server derives sessions
/// from views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RumEventType {
    View,
    Action,
    Error,
    Resource,
    LongTask,
    Vital,
    Log,
}

impl RumEventType {
    pub const ALL: [Self; 7] = [
        Self::View,
        Self::Action,
        Self::Error,
        Self::Resource,
        Self::LongTask,
        Self::Vital,
        Self::Log,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::View => "view",
            Self::Action => "action",
            Self::Error => "error",
            Self::Resource => "resource",
            Self::LongTask => "long_task",
            Self::Vital => "vital",
            Self::Log => "log",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    pub const fn stream(self) -> RumStream {
        match self {
            Self::View => RumStream::Views,
            Self::Action => RumStream::Actions,
            Self::Error => RumStream::Errors,
            Self::Resource => RumStream::Resources,
            Self::LongTask => RumStream::LongTasks,
            Self::Vital => RumStream::Vitals,
            Self::Log => RumStream::Logs,
        }
    }
}

/// The Logs streams RUM data is kept in, one per kind of event, and the sessions derived from the
/// views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RumStream {
    Sessions,
    Views,
    Actions,
    Errors,
    Resources,
    LongTasks,
    Vitals,
    Logs,
}

impl RumStream {
    pub const ALL: [Self; 8] = [
        Self::Sessions,
        Self::Views,
        Self::Actions,
        Self::Errors,
        Self::Resources,
        Self::LongTasks,
        Self::Vitals,
        Self::Logs,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Sessions => "rum_sessions",
            Self::Views => "rum_views",
            Self::Actions => "rum_actions",
            Self::Errors => "rum_errors",
            Self::Resources => "rum_resources",
            Self::LongTasks => "rum_long_tasks",
            Self::Vitals => "rum_vitals",
            Self::Logs => "rum_logs",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|stream| stream.name() == name)
    }

    /// The kind of event the stream holds; `None` for the derived sessions.
    pub const fn event_type(self) -> Option<RumEventType> {
        match self {
            Self::Sessions => None,
            Self::Views => Some(RumEventType::View),
            Self::Actions => Some(RumEventType::Action),
            Self::Errors => Some(RumEventType::Error),
            Self::Resources => Some(RumEventType::Resource),
            Self::LongTasks => Some(RumEventType::LongTask),
            Self::Vitals => Some(RumEventType::Vital),
            Self::Logs => Some(RumEventType::Log),
        }
    }

    /// What the API and the pages call the stream by: its name without the `rum_` prefix
    /// (`views`, `long_tasks`).
    pub fn collection(self) -> &'static str {
        let name = self.name();

        name.strip_prefix("rum_").unwrap_or(name)
    }

    pub fn from_collection(collection: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|stream| stream.collection() == collection)
    }

    /// The column that names one row of the stream to the pages: the session or the view, or the
    /// event itself.
    pub const fn id_field(self) -> &'static str {
        match self {
            Self::Sessions => "session.id",
            Self::Views => "view.id",
            Self::Actions => "action.id",
            Self::Errors => "error.id",
            Self::Resources => "resource.id",
            Self::LongTasks => "long_task.id",
            Self::Vitals => "vital.id",
            Self::Logs => "log.id",
        }
    }

    /// The columns that name one entity and the revision of it, for the streams whose rows are
    /// reported again as the entity changes. Of the rows that share a key only the highest
    /// revision counts.
    pub const fn versioned(self) -> Option<Versioned> {
        match self {
            Self::Views => Some(Versioned {
                key: "view.id",
                revision: "view.revision",
            }),
            Self::Sessions => Some(Versioned {
                key: "session.id",
                revision: "session.revision",
            }),
            _ => None,
        }
    }
}

/// How the rows of one entity are told apart and ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Versioned {
    pub key: &'static str,
    pub revision: &'static str,
}

impl Versioned {
    /// The versioning a set of column names declares, if any. A dataset is versioned by what its
    /// schema holds, not by the stream it belongs to.
    pub fn declared_by<'a>(mut columns: impl FnMut(&str) -> bool + 'a) -> Option<Self> {
        [RumStream::Views, RumStream::Sessions]
            .into_iter()
            .filter_map(RumStream::versioned)
            .find(|versioned| columns(versioned.key) && columns(versioned.revision))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_event_type_has_its_own_stream() {
        let streams = RumEventType::ALL.map(|kind| kind.stream());

        for stream in RumStream::ALL {
            assert_eq!(
                streams.contains(&stream),
                stream != RumStream::Sessions,
                "{stream:?}"
            );
        }

        for kind in RumEventType::ALL {
            assert_eq!(RumEventType::parse(kind.as_str()), Some(kind));
            assert_eq!(kind.stream().event_type(), Some(kind));
        }
    }

    #[test]
    fn stream_names_round_trip() {
        for stream in RumStream::ALL {
            assert_eq!(RumStream::from_name(stream.name()), Some(stream));
        }

        assert_eq!(RumStream::from_name("rum_unknown"), None);
    }

    #[test]
    fn streams_are_called_by_their_name_without_the_prefix() {
        for stream in RumStream::ALL {
            assert_eq!(
                RumStream::from_collection(stream.collection()),
                Some(stream)
            );
        }

        assert_eq!(RumStream::Sessions.collection(), "sessions");
        assert_eq!(RumStream::LongTasks.collection(), "long_tasks");
        assert_eq!(RumStream::from_collection("session"), None);
        assert_eq!(RumStream::from_collection("rum_views"), None);
    }

    #[test]
    fn every_stream_names_its_rows_by_a_column_it_has() {
        use crate::domain::rum::event::field;

        for stream in RumStream::ALL {
            assert!(field(stream, stream.id_field()).is_some(), "{stream:?}");
        }
    }

    #[test]
    fn only_views_and_sessions_are_versioned() {
        for stream in RumStream::ALL {
            assert_eq!(
                stream.versioned().is_some(),
                matches!(stream, RumStream::Views | RumStream::Sessions)
            );
        }
    }

    #[test]
    fn versioning_is_declared_by_the_columns_a_dataset_has() {
        let views = Versioned::declared_by(|name| matches!(name, "view.id" | "view.revision"));
        assert_eq!(views.map(|versioned| versioned.key), Some("view.id"));

        let sessions =
            Versioned::declared_by(|name| matches!(name, "session.id" | "session.revision"));
        assert_eq!(sessions.map(|versioned| versioned.key), Some("session.id"));

        assert_eq!(Versioned::declared_by(|name| name == "view.id"), None);
        assert_eq!(Versioned::declared_by(|_| false), None);
    }
}
