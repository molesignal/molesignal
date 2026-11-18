// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};
use svix_ksuid::{Ksuid, KsuidLike};
use uuid::Uuid;

/// Globally sort-friendly ID (KSUID by default).
///
/// Stored internally as a `String`, which can hold either a KSUID (the default) or UUID text
/// (hyphenated). To convert to and from sea-orm's `Uuid` primary keys, three helper methods are
/// provided: [`Id::from_uuid`] / [`Id::as_uuid`] / [`Id::new_uuid`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub String);

impl Id {
    /// Generates a KSUID by default: 27 characters, and lexicographic order
    /// approximates time order.
    pub fn new() -> Self {
        // svix-ksuid 0.10 requires a Timestamp type annotation; pass None so
        // the crate picks "now" via its default impl on SystemTime.
        Self(Ksuid::new(None::<std::time::SystemTime>, None).to_string())
    }

    /// Generates a time-sortable UUID v7, stored as a hyphenated string.
    /// Prefer this when the target storage is a Postgres `UUID` column.
    pub fn new_uuid() -> Self {
        Self(Uuid::now_v7().to_string())
    }

    pub fn from_string(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Wraps a [`uuid::Uuid`] as an `Id` (hyphenated string).
    pub fn from_uuid(u: Uuid) -> Self {
        Self(u.to_string())
    }

    /// Parses the internal string as a UUID.
    /// Returns `None` for KSUID strings (their length and character set differ from a UUID's).
    pub fn as_uuid(&self) -> Option<Uuid> {
        Uuid::parse_str(&self.0).ok()
    }

    /// Forces the value to be read as a UUID and panics on failure; only for entry points that
    /// are "known to be a UUID" (such as sea-orm primary key mapping).
    pub fn expect_uuid(&self) -> Uuid {
        self.as_uuid()
            .unwrap_or_else(|| panic!("Id {:?} is not a valid UUID", self.0))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Id {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<Uuid> for Id {
    fn from(u: Uuid) -> Self {
        Self::from_uuid(u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ksuid_constructor_is_27_chars() {
        let id = Id::new();
        assert_eq!(id.as_str().len(), 27);
        assert!(id.as_uuid().is_none(), "KSUID should not parse as UUID");
    }

    #[test]
    fn uuid_roundtrip() {
        let u = Uuid::now_v7();
        let id = Id::from_uuid(u);
        assert_eq!(id.as_uuid(), Some(u));
        assert_eq!(id.expect_uuid(), u);
    }

    #[test]
    fn new_uuid_parses_back() {
        let id = Id::new_uuid();
        let u = id.as_uuid().expect("UUID Id must parse");
        // v7 sets version nibble to 7
        assert_eq!(u.get_version_num(), 7);
    }

    #[test]
    fn from_trait_works() {
        let u = Uuid::now_v7();
        let id: Id = u.into();
        assert_eq!(id.as_uuid(), Some(u));
    }
}
