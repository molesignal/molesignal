// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The versions of an application, in the order they were first seen.
//!
//! What tells a newer version from an older one is not its name (a version may be a number, a date
//! or a commit) but which was seen first. Every event of every type carries a version, so the
//! ledger is kept where events are received, on any node; a version is written once, and the
//! node remembers it has been.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use moka::sync::Cache;

use crate::{
    domain::rum::{RumVersionRepository, version_key},
    shared::{ids::Id, time::TimestampMicros},
};

/// Versions remembered as written, and for how long. A version dropped from memory is written
/// again, which the ledger takes as nothing new.
const REMEMBERED: u64 = 100_000;
const REMEMBERED_FOR: Duration = Duration::from_secs(60 * 60);

pub struct VersionLedger {
    repository: Arc<dyn RumVersionRepository>,
    written: Cache<(Id, String, String), ()>,
}

impl VersionLedger {
    pub fn new(repository: Arc<dyn RumVersionRepository>) -> Self {
        Self {
            repository,
            written: Cache::builder()
                .max_capacity(REMEMBERED)
                .time_to_live(REMEMBERED_FOR)
                .build(),
        }
    }

    /// Notes the versions events were received with, each with its application. A ledger that
    /// cannot be written is not a reason to refuse the events: it is logged, and the versions are
    /// noted again with the next events that carry them.
    pub async fn note<'a>(
        &self,
        org_id: &Id,
        seen: impl IntoIterator<Item = (&'a str, &'a str)>,
        at: TimestampMicros,
    ) {
        let mut fresh: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();

        for (application, version) in seen {
            let Some(version) = version_key(version) else {
                continue;
            };

            let key = (org_id.clone(), application.to_string(), version.to_string());

            if !self.written.contains_key(&key) {
                fresh.entry(application).or_default().insert(version);
            }
        }

        for (application, versions) in fresh {
            let versions = versions.into_iter().map(str::to_string).collect::<Vec<_>>();

            match self
                .repository
                .record(org_id, application, &versions, at)
                .await
            {
                Ok(()) => {
                    for version in versions {
                        self.written
                            .insert((org_id.clone(), application.to_string(), version), ());
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, application, "RUM versions could not be noted");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use async_trait::async_trait;
    use parking_lot::Mutex;

    use super::*;
    use crate::shared::Result;

    #[derive(Default)]
    struct Recording {
        calls: Mutex<Vec<(String, Vec<String>)>>,
        fail: Mutex<bool>,
    }

    #[async_trait]
    impl RumVersionRepository for Recording {
        async fn record(
            &self,
            _org_id: &Id,
            application_id: &str,
            versions: &[String],
            _at: TimestampMicros,
        ) -> Result<()> {
            if *self.fail.lock() {
                return Err(crate::shared::Error::unavailable("down"));
            }

            self.calls
                .lock()
                .push((application_id.to_string(), versions.to_vec()));

            Ok(())
        }

        async fn first_seen(
            &self,
            _org_id: &Id,
            _application_id: &str,
            _versions: &[String],
        ) -> Result<HashMap<String, TimestampMicros>> {
            Ok(HashMap::new())
        }
    }

    #[tokio::test]
    async fn a_version_is_noted_once_per_application() {
        let repository = Arc::new(Recording::default());
        let ledger = VersionLedger::new(repository.clone());
        let org = Id::from_string("org");

        let events = [
            ("web", "2.4.1"),
            ("web", "2.4.1"),
            ("web", "2.4.0"),
            ("app", "1.0.0"),
            ("web", "  "),
        ];

        ledger.note(&org, events, TimestampMicros(1)).await;
        ledger.note(&org, events, TimestampMicros(2)).await;

        let mut calls = repository.calls.lock().clone();

        calls.sort();

        assert_eq!(
            calls,
            vec![
                ("app".to_string(), vec!["1.0.0".to_string()]),
                (
                    "web".to_string(),
                    vec!["2.4.0".to_string(), "2.4.1".to_string()]
                ),
            ]
        );
    }

    #[tokio::test]
    async fn a_version_that_could_not_be_noted_is_noted_again_later() {
        let repository = Arc::new(Recording::default());
        let ledger = VersionLedger::new(repository.clone());
        let org = Id::from_string("org");

        *repository.fail.lock() = true;
        ledger
            .note(&org, [("web", "2.4.1")], TimestampMicros(1))
            .await;
        assert!(repository.calls.lock().is_empty());

        *repository.fail.lock() = false;
        ledger
            .note(&org, [("web", "2.4.1")], TimestampMicros(2))
            .await;
        assert_eq!(repository.calls.lock().len(), 1);
    }
}
