// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Keeping the issues the errors belong to.
//!
//! The intake of `rum_errors` is one node's for an organization, so this is the one place that
//! sees every error: which issues are new, which come back, and how many an application has
//! opened this hour. A request cannot know that, which is why the id of an error's issue is only
//! worked out from the error there, and the rest is settled here.
//!
//! What is known of an issue is remembered for a minute, so most errors cost no read. A new issue
//! is written at once. An issue seen again is written at most once a minute, unless it is seen in
//! a new version. A resolved issue seen in a version newer than the one it was resolved in is
//! moved to regressed at once, with the environment of the error that brought it back. An error in
//! a version the issue was not seen in before is also the one that would bring it back, so the
//! issue is read again for it: a person may have resolved it on another node within the minute.
//! The first time an issue turns up in an environment is written once: that is what "new in
//! production" is counted from.

mod seen;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use moka::sync::Cache;
use parking_lot::Mutex;
use serde_json::Value;

use self::seen::{Seen, gather, merged, opening};
use super::identity::overflow_id;
use crate::{
    domain::{
        intake::RawEvent,
        rum::{
            DEFAULT_ISSUES_PER_HOUR, RumApplicationRepository, RumIssue, RumIssueEnvironment,
            RumIssueRepository, RumIssueSighting, RumIssueTracker, RumVersionRepository,
        },
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// Issues remembered, and for how long. What a person changes on another node reaches this one
/// within it.
const KNOWN_ISSUES: u64 = 100_000;
const KNOWN_FOR: Duration = Duration::from_secs(60);

/// How long what was last written of an issue is remembered; longer than what is known of it, so
/// that an issue read again is not written again at once.
const WRITTEN_FOR: Duration = Duration::from_secs(10 * 60);

/// How long what an application may open in an hour is remembered.
const LIMIT_FOR: Duration = Duration::from_secs(30);

/// An issue seen again is written at most this often, but for a new version.
const TOUCH_EVERY_MICROS: i64 = 60_000_000;

const HOUR_MICROS: i64 = 3_600_000_000;

/// How many (issue, environment) pairs are remembered as written. A pair that falls out is
/// written again, which changes nothing.
const KNOWN_ENVIRONMENTS: u64 = 200_000;

pub struct IssueTracker {
    issues: Arc<dyn RumIssueRepository>,
    versions: Arc<dyn RumVersionRepository>,
    applications: Arc<dyn RumApplicationRepository>,
    known: Cache<(Id, String), RumIssue>,
    /// When and in which version an issue was last written as seen. Kept apart from what is known
    /// of the issue: writing a sighting must not renew that, or an issue that keeps happening
    /// would never be read again and a person's change to it would never be noticed.
    written: Cache<(Id, String), (i64, Option<String>)>,
    /// The versions an issue was read again for, so that a version costs one read, not one for
    /// each of its errors.
    rechecked: Cache<(Id, String, String), ()>,
    limits: Cache<(Id, String), u32>,
    /// When a version was first seen never changes, so it is remembered for good.
    firsts: Cache<(Id, String, String), TimestampMicros>,
    /// The environments an issue is known to have been written for.
    environments: Cache<(Id, String, String), ()>,
    opened: Mutex<HashMap<(Id, String), Opened>>,
}

/// How many issues an application has opened in an hour.
struct Opened {
    hour: i64,
    count: u32,
}

impl IssueTracker {
    pub fn new(
        issues: Arc<dyn RumIssueRepository>,
        versions: Arc<dyn RumVersionRepository>,
        applications: Arc<dyn RumApplicationRepository>,
    ) -> Self {
        Self {
            issues,
            versions,
            applications,
            known: Cache::builder()
                .max_capacity(KNOWN_ISSUES)
                .time_to_live(KNOWN_FOR)
                .build(),
            written: Cache::builder()
                .max_capacity(KNOWN_ISSUES)
                .time_to_live(WRITTEN_FOR)
                .build(),
            rechecked: Cache::builder()
                .max_capacity(KNOWN_ISSUES)
                .time_to_live(WRITTEN_FOR)
                .build(),
            limits: Cache::builder()
                .max_capacity(10_000)
                .time_to_live(LIMIT_FOR)
                .build(),
            firsts: Cache::builder().max_capacity(100_000).build(),
            environments: Cache::builder().max_capacity(KNOWN_ENVIRONMENTS).build(),
            opened: Mutex::default(),
        }
    }

    async fn track_rows(
        &self,
        org_id: &Id,
        rows: &mut [RawEvent],
        now: TimestampMicros,
    ) -> Result<()> {
        let mut seen = gather(rows);

        if seen.is_empty() {
            return Ok(());
        }

        self.load(org_id, &seen).await?;

        // What an application opens past what it may in an hour is counted as one issue.
        let mut redirected = false;

        for item in &mut seen {
            if self.is_known(org_id, &item.id) || item.id == overflow_id(&item.application) {
                continue;
            }

            let limit = self.limit(org_id, &item.application).await;

            if self.admit(org_id, &item.application, now, limit) {
                continue;
            }

            let rest = overflow_id(&item.application);

            for &index in &item.rows {
                rows[index]
                    .fields
                    .insert("error.issue_id".into(), Value::String(rest.clone()));
            }

            item.id = rest;
            redirected = true;
        }

        if redirected {
            seen = merged(seen);
            self.load(org_id, &seen).await?;
        }

        let fresh = self.open(org_id, &seen, rows).await?;

        // Where an issue turned up is a record of its own: failing to write it must not keep the
        // issue from being followed.
        if let Err(error) = self.note_environments(org_id, &seen).await {
            tracing::warn!(%error, "RUM issue environments could not be written");
        }

        self.follow(org_id, &seen, &fresh).await
    }

    /// Opens the issues nobody has seen before, and remembers them.
    async fn open(&self, org_id: &Id, seen: &[Seen], rows: &[RawEvent]) -> Result<HashSet<String>> {
        let new = seen
            .iter()
            .filter(|item| !self.is_known(org_id, &item.id))
            .map(|item| opening(item, rows))
            .collect::<Vec<_>>();

        if new.is_empty() {
            return Ok(HashSet::new());
        }

        let ids = new.iter().map(|issue| issue.id.clone()).collect();

        for issue in self.issues.open(org_id, &new).await? {
            self.remember(org_id, issue);
        }

        Ok(ids)
    }

    /// Writes the environments issues were seen in that are not known to be written.
    async fn note_environments(&self, org_id: &Id, seen: &[Seen]) -> Result<()> {
        let mut unwritten = Vec::new();

        for item in seen {
            for (environment, at) in &item.environments {
                let key = (org_id.clone(), item.id.clone(), environment.clone());

                if self.environments.contains_key(&key) {
                    continue;
                }

                unwritten.push(RumIssueEnvironment {
                    issue_id: item.id.clone(),
                    application_id: item.application.clone(),
                    environment: environment.clone(),
                    first_seen_at: TimestampMicros(*at),
                });
            }
        }

        if unwritten.is_empty() {
            return Ok(());
        }

        self.issues.record_environments(org_id, &unwritten).await?;

        for written in unwritten {
            self.environments
                .insert((org_id.clone(), written.issue_id, written.environment), ());
        }

        Ok(())
    }

    /// Reads again the issues that are seen in a version they were not last seen in, once for each
    /// version. What a node knows of an issue is up to a minute old; a person who resolved it
    /// meanwhile on another node is heard of here, before the error that brings it back is judged
    /// by a copy that says it is open. An issue that cannot be read again is judged as it was.
    async fn refresh_for_new_versions(&self, org_id: &Id, seen: &[Seen], fresh: &HashSet<String>) {
        let mut stale = Vec::new();

        for item in seen.iter().filter(|item| !fresh.contains(&item.id)) {
            let key = (org_id.clone(), item.id.clone());

            let Some(issue) = self.known.get(&key) else {
                continue;
            };

            // The version it was last seen in is what was written last, when something was.
            let last = match self.written.get(&key) {
                Some((at, version)) if at >= issue.last_seen_at.0 => version,
                _ => issue.last_version.clone(),
            };

            let mut new = false;

            for version in item
                .sightings
                .iter()
                .filter_map(|sighting| sighting.version.as_ref())
                .filter(|version| last.as_ref() != Some(*version))
            {
                let looked = (org_id.clone(), item.id.clone(), version.clone());

                if !self.rechecked.contains_key(&looked) {
                    self.rechecked.insert(looked, ());
                    new = true;
                }
            }

            if new {
                stale.push(item.id.clone());
            }
        }

        if stale.is_empty() {
            return;
        }

        match self.issues.get_many(org_id, &stale).await {
            Ok(issues) => {
                for issue in issues {
                    self.remember(org_id, issue);
                }
            }
            Err(error) => {
                tracing::warn!(%error, "RUM issues could not be read again for a new version");
            }
        }
    }

    /// What became of the issues that were there before: seen again, and perhaps back.
    async fn follow(&self, org_id: &Id, seen: &[Seen], fresh: &HashSet<String>) -> Result<()> {
        self.refresh_for_new_versions(org_id, seen, fresh).await;

        let firsts = self.first_seen(org_id, seen, fresh).await;

        let mut touches = Vec::new();
        let mut writes = Vec::new();

        for item in seen.iter().filter(|item| !fresh.contains(&item.id)) {
            let Some(issue) = self.known.get(&(org_id.clone(), item.id.clone())) else {
                continue;
            };

            let reopening = item.sightings.iter().find(|sighting| {
                issue.is_reopened_by(
                    TimestampMicros(sighting.at),
                    sighting.version.as_deref(),
                    |version| {
                        firsts
                            .get(&(item.application.clone(), version.to_string()))
                            .copied()
                    },
                )
            });

            let reopened = reopening.zip(issue.resolved_at);

            if let Some((sighting, resolved_at)) = reopened {
                self.reopen(
                    org_id,
                    &item.id,
                    resolved_at,
                    sighting.at,
                    sighting.version.as_deref(),
                    sighting.environment.as_deref(),
                )
                .await;
            }

            // What was written last is what throttles, and what the issue was read with when
            // nothing was written since.
            let key = (org_id.clone(), item.id.clone());
            let (last_at, last_version) = match self.written.get(&key) {
                Some((at, version)) if at >= issue.last_seen_at.0 => (at, version),
                _ => (issue.last_seen_at.0, issue.last_version.clone()),
            };

            let newer_version = item.last_version.is_some() && item.last_version != last_version;

            // The errors that bring an issue back are its latest sightings, however recent the
            // last write: the issue must not read as last seen before it came back.
            if reopened.is_some() || newer_version || item.last_at - last_at >= TOUCH_EVERY_MICROS {
                touches.push(RumIssueSighting {
                    id: item.id.clone(),
                    at: TimestampMicros(item.last_at),
                    version: item.last_version.clone(),
                });

                writes.push((
                    key,
                    (
                        item.last_at.max(last_at),
                        item.last_version.clone().or(last_version),
                    ),
                ));
            }
        }

        if touches.is_empty() {
            return Ok(());
        }

        self.issues.record_sightings(org_id, &touches).await?;

        for (key, written) in writes {
            self.written.insert(key, written);
        }

        Ok(())
    }

    async fn reopen(
        &self,
        org_id: &Id,
        id: &str,
        resolved_at: TimestampMicros,
        at: i64,
        version: Option<&str>,
        environment: Option<&str>,
    ) {
        let key = (org_id.clone(), id.to_string());

        match self
            .issues
            .mark_regressed(
                org_id,
                id,
                resolved_at,
                TimestampMicros(at),
                version,
                environment,
            )
            .await
        {
            Ok(Some(issue)) => self.remember(org_id, issue),
            // Changed by a person since it was read: read again when it is next seen.
            Ok(None) => self.known.invalidate(&key),
            Err(error) => {
                tracing::warn!(%error, issue = id, "RUM issue could not be moved to regressed");
                self.known.invalidate(&key);
            }
        }
    }

    /// When the versions that matter were first seen: those a resolved issue was resolved in, and
    /// those it is seen in now. Nothing is asked for when no resolved issue is seen.
    async fn first_seen(
        &self,
        org_id: &Id,
        seen: &[Seen],
        fresh: &HashSet<String>,
    ) -> HashMap<(String, String), TimestampMicros> {
        let mut wanted: HashMap<&str, HashSet<String>> = HashMap::new();

        for item in seen.iter().filter(|item| !fresh.contains(&item.id)) {
            let Some(issue) = self.known.get(&(org_id.clone(), item.id.clone())) else {
                continue;
            };

            if issue.resolved_at.is_none() || issue.resolved_version.is_none() {
                continue;
            }

            let versions = wanted.entry(item.application.as_str()).or_default();

            versions.extend(issue.resolved_version);
            versions.extend(
                item.sightings
                    .iter()
                    .filter_map(|sighting| sighting.version.clone()),
            );
        }

        let mut firsts = HashMap::new();

        for (application, versions) in wanted {
            let mut missing = Vec::new();

            for version in versions {
                let key = (org_id.clone(), application.to_string(), version.clone());

                match self.firsts.get(&key) {
                    Some(first) => {
                        firsts.insert((application.to_string(), version), first);
                    }
                    None => missing.push(version),
                }
            }

            if missing.is_empty() {
                continue;
            }

            match self
                .versions
                .first_seen(org_id, application, &missing)
                .await
            {
                Ok(found) => {
                    for (version, first) in found {
                        self.firsts.insert(
                            (org_id.clone(), application.to_string(), version.clone()),
                            first,
                        );
                        firsts.insert((application.to_string(), version), first);
                    }
                }
                // Without them nothing is judged to be back: it is judged when they can be read.
                Err(error) => {
                    tracing::warn!(%error, application, "RUM versions could not be read");
                }
            }
        }

        firsts
    }

    /// Reads the issues of `seen` that are not remembered.
    async fn load(&self, org_id: &Id, seen: &[Seen]) -> Result<()> {
        let missing = seen
            .iter()
            .filter(|item| !self.is_known(org_id, &item.id))
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();

        if missing.is_empty() {
            return Ok(());
        }

        for issue in self.issues.get_many(org_id, &missing).await? {
            self.remember(org_id, issue);
        }

        Ok(())
    }

    fn is_known(&self, org_id: &Id, id: &str) -> bool {
        self.known.contains_key(&(org_id.clone(), id.to_string()))
    }

    /// Keeps what is needed of an issue to judge it again: not the words it is shown with.
    fn remember(&self, org_id: &Id, mut issue: RumIssue) {
        issue.message = String::new();
        issue.location = String::new();
        issue.error_type = String::new();
        issue.source = String::new();

        self.known.insert((org_id.clone(), issue.id.clone()), issue);
    }

    /// What an application may open in an hour.
    async fn limit(&self, org_id: &Id, application: &str) -> u32 {
        let key = (org_id.clone(), application.to_string());

        if let Some(limit) = self.limits.get(&key) {
            return limit;
        }

        let limit = match self.applications.get(org_id, application).await {
            Ok(found) => found.settings.issues_per_hour,
            Err(error) => {
                tracing::debug!(%error, application, "RUM application limit not read; using the default");
                DEFAULT_ISSUES_PER_HOUR
            }
        };

        self.limits.insert(key, limit);

        limit
    }

    /// Counts one more issue opened by the application this hour, if it may open one.
    fn admit(&self, org_id: &Id, application: &str, now: TimestampMicros, limit: u32) -> bool {
        let hour = now.0 / HOUR_MICROS;
        let mut opened = self.opened.lock();

        if opened.len() > 10_000 {
            opened.retain(|_, counted| counted.hour == hour);
        }

        let counted = opened
            .entry((org_id.clone(), application.to_string()))
            .or_insert(Opened { hour, count: 0 });

        if counted.hour != hour {
            *counted = Opened { hour, count: 0 };
        }

        if counted.count >= limit {
            return false;
        }

        counted.count += 1;

        true
    }
}

#[async_trait]
impl RumIssueTracker for IssueTracker {
    async fn track(&self, org_id: &Id, rows: &mut [RawEvent]) {
        if let Err(error) = self.track_rows(org_id, rows, TimestampMicros::now()).await {
            tracing::warn!(%error, "RUM issues could not be tracked; the errors are written as they are");
        }
    }
}

#[cfg(test)]
mod tests;
