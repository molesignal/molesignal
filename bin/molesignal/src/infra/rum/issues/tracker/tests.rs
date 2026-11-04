// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use serde_json::json;

use super::*;
use crate::{
    domain::rum::{
        RumApplication, RumApplicationPatch, RumApplicationSettings, RumIssueChange,
        RumIssueMoment, RumIssueStatus, RumPlatform,
    },
    shared::Error,
};

#[derive(Default)]
struct Issues {
    kept: Mutex<HashMap<String, RumIssue>>,
    opened: Mutex<Vec<Vec<String>>>,
    sightings: Mutex<Vec<Vec<RumIssueSighting>>>,
    /// Each write of environments, as (issue, environment, time).
    environments: Mutex<Vec<Vec<(String, String, i64)>>>,
    unreadable: Mutex<bool>,
    /// How many times issues were read, in batches.
    reads: Mutex<usize>,
}

#[async_trait]
impl RumIssueRepository for Issues {
    async fn get(&self, _org_id: &Id, id: &str) -> Result<Option<RumIssue>> {
        Ok(self.kept.lock().get(id).cloned())
    }

    async fn get_many(&self, _org_id: &Id, ids: &[String]) -> Result<Vec<RumIssue>> {
        *self.reads.lock() += 1;

        if *self.unreadable.lock() {
            return Err(Error::unavailable("the database is down"));
        }

        let kept = self.kept.lock();

        Ok(ids.iter().filter_map(|id| kept.get(id).cloned()).collect())
    }

    async fn open(&self, _org_id: &Id, issues: &[RumIssue]) -> Result<Vec<RumIssue>> {
        self.opened
            .lock()
            .push(issues.iter().map(|issue| issue.id.clone()).collect());

        let mut kept = self.kept.lock();

        Ok(issues
            .iter()
            .map(|issue| {
                kept.entry(issue.id.clone())
                    .or_insert_with(|| issue.clone())
                    .clone()
            })
            .collect())
    }

    async fn record_sightings(&self, _org_id: &Id, sightings: &[RumIssueSighting]) -> Result<()> {
        self.sightings.lock().push(sightings.to_vec());

        let mut kept = self.kept.lock();

        for sighting in sightings {
            if let Some(issue) = kept.get_mut(&sighting.id) {
                issue.last_seen_at = issue.last_seen_at.max(sighting.at);
                issue.last_version = sighting.version.clone().or(issue.last_version.take());
            }
        }

        Ok(())
    }

    async fn record_environments(
        &self,
        _org_id: &Id,
        environments: &[RumIssueEnvironment],
    ) -> Result<()> {
        self.environments.lock().push(
            environments
                .iter()
                .map(|seen| {
                    (
                        seen.issue_id.clone(),
                        seen.environment.clone(),
                        seen.first_seen_at.0,
                    )
                })
                .collect(),
        );

        Ok(())
    }

    async fn mark_regressed(
        &self,
        _org_id: &Id,
        id: &str,
        resolved_at: TimestampMicros,
        at: TimestampMicros,
        version: Option<&str>,
        environment: Option<&str>,
    ) -> Result<Option<RumIssue>> {
        let mut kept = self.kept.lock();

        let Some(issue) = kept.get_mut(id) else {
            return Ok(None);
        };

        if issue.status != RumIssueStatus::Resolved || issue.resolved_at != Some(resolved_at) {
            return Ok(None);
        }

        issue.status = RumIssueStatus::Regressed;
        issue.regressed_at = Some(at);
        issue.regressed_version = version.map(str::to_string);
        issue.regressed_environment = environment.map(str::to_string);

        Ok(Some(issue.clone()))
    }

    async fn first_seen_between(
        &self,
        _org_id: &Id,
        _application_id: &str,
        _environment: Option<&str>,
        _from: TimestampMicros,
        _to: TimestampMicros,
        _limit: usize,
    ) -> Result<Vec<RumIssueMoment>> {
        Ok(Vec::new())
    }

    async fn regressed_between(
        &self,
        _org_id: &Id,
        _application_id: &str,
        _environment: Option<&str>,
        _from: TimestampMicros,
        _to: TimestampMicros,
        _limit: usize,
    ) -> Result<Vec<RumIssueMoment>> {
        Ok(Vec::new())
    }

    async fn update(
        &self,
        _org_id: &Id,
        _id: &str,
        _change: &RumIssueChange,
        _by: &Id,
        _at: TimestampMicros,
    ) -> Result<Option<RumIssue>> {
        Ok(None)
    }
}

#[derive(Default)]
struct Versions {
    firsts: HashMap<String, i64>,
}

#[async_trait]
impl RumVersionRepository for Versions {
    async fn record(
        &self,
        _org_id: &Id,
        _application_id: &str,
        _versions: &[String],
        _at: TimestampMicros,
    ) -> Result<()> {
        Ok(())
    }

    async fn first_seen(
        &self,
        _org_id: &Id,
        _application_id: &str,
        versions: &[String],
    ) -> Result<HashMap<String, TimestampMicros>> {
        Ok(versions
            .iter()
            .filter_map(|version| {
                self.firsts
                    .get(version)
                    .map(|first| (version.clone(), TimestampMicros(*first)))
            })
            .collect())
    }
}

struct Applications {
    limit: u32,
}

#[async_trait]
impl RumApplicationRepository for Applications {
    async fn create(&self, application: RumApplication) -> Result<RumApplication> {
        Ok(application)
    }

    async fn get(&self, org_id: &Id, id: &str) -> Result<RumApplication> {
        Ok(RumApplication {
            org_id: org_id.clone(),
            id: id.to_string(),
            name: id.to_string(),
            platform: RumPlatform::Browser,
            settings: RumApplicationSettings {
                issues_per_hour: self.limit,
                ..Default::default()
            },
            created_by: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        })
    }

    async fn list(&self, _org_id: &Id) -> Result<Vec<RumApplication>> {
        Ok(Vec::new())
    }

    async fn update(
        &self,
        org_id: &Id,
        id: &str,
        _patch: RumApplicationPatch,
    ) -> Result<RumApplication> {
        self.get(org_id, id).await
    }

    async fn delete(&self, _org_id: &Id, _id: &str) -> Result<()> {
        Ok(())
    }
}

const NOW: i64 = 1_760_000_000_000_000;
const MINUTE: i64 = 60_000_000;

fn tracker(limit: u32, firsts: &[(&str, i64)]) -> (IssueTracker, Arc<Issues>) {
    let issues = Arc::new(Issues::default());
    let versions = Arc::new(Versions {
        firsts: firsts
            .iter()
            .map(|(version, first)| (version.to_string(), *first))
            .collect(),
    });

    (
        IssueTracker::new(issues.clone(), versions, Arc::new(Applications { limit })),
        issues,
    )
}

fn error(issue: &str, version: &str, at: i64) -> RawEvent {
    RawEvent {
        timestamp: TimestampMicros(at),
        fields: json!({
            "application.id": "shop-web",
            "error.issue_id": issue,
            "version": version,
            "error.source": "source",
            "error.type": "TypeError",
            "error.message": "x is undefined",
        })
        .as_object()
        .unwrap()
        .clone(),
    }
}

/// An error that happened in an environment.
fn error_in(issue: &str, version: &str, environment: &str, at: i64) -> RawEvent {
    let mut row = error(issue, version, at);

    row.fields.insert("env".into(), json!(environment));

    row
}

fn org() -> Id {
    Id::from_string("org")
}

async fn track(tracker: &IssueTracker, rows: &mut [RawEvent], now: i64) {
    tracker
        .track_rows(&org(), rows, TimestampMicros(now))
        .await
        .expect("tracked");
}

fn issue_of(row: &RawEvent) -> String {
    row.fields["error.issue_id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn a_new_issue_is_opened_once_with_what_its_first_error_says() {
    let (tracker, issues) = tracker(100, &[]);

    let mut rows = vec![error("a", "2.4.1", NOW), error("a", "2.4.1", NOW + 1_000)];

    track(&tracker, &mut rows, NOW).await;
    track(&tracker, &mut rows, NOW + 1_000).await;

    assert_eq!(*issues.opened.lock(), vec![vec!["a".to_string()]]);

    let kept = issues.kept.lock()["a"].clone();

    assert_eq!(kept.error_type, "TypeError");
    assert_eq!(kept.message, "x is undefined");
    assert_eq!(kept.first_version.as_deref(), Some("2.4.1"));
    assert_eq!(kept.first_seen_at, TimestampMicros(NOW));
    assert_eq!(kept.last_seen_at, TimestampMicros(NOW + 1_000));
    assert_eq!(kept.status, RumIssueStatus::Open);
    assert!(issues.sightings.lock().is_empty());
}

#[tokio::test]
async fn an_issue_seen_again_is_written_once_a_minute() {
    let (tracker, issues) = tracker(100, &[]);

    track(&tracker, &mut [error("a", "2.4.1", NOW)], NOW).await;
    track(&tracker, &mut [error("a", "2.4.1", NOW + 30_000_000)], NOW).await;
    assert!(issues.sightings.lock().is_empty());

    track(&tracker, &mut [error("a", "2.4.1", NOW + MINUTE)], NOW).await;

    let written = issues.sightings.lock().clone();

    assert_eq!(written.len(), 1);
    assert_eq!(written[0][0].at, TimestampMicros(NOW + MINUTE));

    // What was written is remembered apart: it does not renew what is known of the issue.
    let key = (org(), "a".to_string());

    assert_eq!(
        tracker.known.get(&key).unwrap().last_seen_at,
        TimestampMicros(NOW)
    );
    assert_eq!(tracker.written.get(&key).unwrap().0, NOW + MINUTE);
}

#[tokio::test]
async fn an_issue_seen_in_a_new_version_is_written_at_once() {
    let (tracker, issues) = tracker(100, &[]);

    track(&tracker, &mut [error("a", "2.4.0", NOW)], NOW).await;
    track(&tracker, &mut [error("a", "2.4.1", NOW + 1_000)], NOW).await;

    let written = issues.sightings.lock().clone();

    assert_eq!(written.len(), 1);
    assert_eq!(written[0][0].version.as_deref(), Some("2.4.1"));
}

#[tokio::test]
async fn what_an_application_opens_past_its_limit_is_counted_as_one_issue() {
    let (tracker, issues) = tracker(2, &[]);

    let mut rows = vec![
        error("a", "2.4.1", NOW),
        error("b", "2.4.1", NOW),
        error("c", "2.4.1", NOW),
        error("d", "2.4.1", NOW),
    ];

    track(&tracker, &mut rows, NOW).await;

    let rest = overflow_id("shop-web");

    assert_eq!(
        rows.iter().map(issue_of).collect::<Vec<_>>(),
        vec!["a".to_string(), "b".to_string(), rest.clone(), rest.clone()]
    );

    let kept = issues.kept.lock();

    assert_eq!(kept.len(), 3);
    assert!(kept[&rest].overflow);
    assert!(!kept["a"].overflow);
}

#[tokio::test]
async fn the_limit_is_counted_for_each_hour_and_each_application() {
    let (tracker, _issues) = tracker(1, &[]);

    let mut first = vec![error("a", "1", NOW), error("b", "1", NOW)];

    track(&tracker, &mut first, NOW).await;
    assert_eq!(issue_of(&first[1]), overflow_id("shop-web"));

    let mut later = vec![error("c", "1", NOW + 2 * HOUR_MICROS)];

    track(&tracker, &mut later, NOW + 2 * HOUR_MICROS).await;
    assert_eq!(issue_of(&later[0]), "c");
}

fn resolved(id: &str, version: Option<&str>) -> RumIssue {
    RumIssue {
        status: RumIssueStatus::Resolved,
        resolved_at: Some(TimestampMicros(NOW - 10 * MINUTE)),
        resolved_version: version.map(str::to_string),
        last_seen_at: TimestampMicros(NOW - 20 * MINUTE),
        ..RumIssue::opened(
            id.to_string(),
            "shop-web".into(),
            TimestampMicros(NOW - 60 * MINUTE),
            Some("2.2.0".into()),
        )
    }
}

#[tokio::test]
async fn a_resolved_issue_seen_in_a_newer_version_regresses() {
    let (tracker, issues) = tracker(100, &[("2.3.0", 100), ("2.4.1", 200)]);

    issues
        .kept
        .lock()
        .insert("a".into(), resolved("a", Some("2.3.0")));

    track(&tracker, &mut [error("a", "2.4.1", NOW)], NOW).await;

    let kept = issues.kept.lock()["a"].clone();

    assert_eq!(kept.status, RumIssueStatus::Regressed);
    assert_eq!(kept.regressed_version.as_deref(), Some("2.4.1"));
    assert_eq!(kept.regressed_at, Some(TimestampMicros(NOW)));
    assert!(issues.opened.lock().is_empty());
}

#[tokio::test]
async fn the_newest_error_says_where_an_issue_that_came_back_was_last_seen() {
    let (tracker, issues) = tracker(100, &[("2.3.0", 100), ("2.4.1", 200), ("2.5.0", 300)]);

    issues
        .kept
        .lock()
        .insert("a".into(), resolved("a", Some("2.3.0")));

    let mut rows = vec![error("a", "2.4.1", NOW), error("a", "2.5.0", NOW + 1_000)];

    track(&tracker, &mut rows, NOW).await;

    let kept = issues.kept.lock()["a"].clone();

    assert_eq!(kept.status, RumIssueStatus::Regressed);
    assert_eq!(kept.regressed_version.as_deref(), Some("2.4.1"));
    assert_eq!(kept.last_version.as_deref(), Some("2.5.0"));
    assert_eq!(kept.last_seen_at, TimestampMicros(NOW + 1_000));
}

#[tokio::test]
async fn the_error_that_brings_an_issue_back_is_written_within_the_minute() {
    let (tracker, issues) = tracker(100, &[]);

    issues.kept.lock().insert(
        "a".into(),
        RumIssue {
            resolved_at: Some(TimestampMicros(NOW - 1_000)),
            last_seen_at: TimestampMicros(NOW - 2_000),
            ..resolved("a", None)
        },
    );

    track(&tracker, &mut [error("a", "2.2.0", NOW)], NOW).await;

    let kept = issues.kept.lock()["a"].clone();

    assert_eq!(kept.status, RumIssueStatus::Regressed);
    assert_eq!(kept.last_seen_at, TimestampMicros(NOW));
}

#[tokio::test]
async fn a_resolved_issue_seen_in_the_version_it_was_resolved_in_stays_resolved() {
    let (tracker, issues) = tracker(100, &[("2.3.0", 100), ("2.4.1", 200)]);

    issues
        .kept
        .lock()
        .insert("a".into(), resolved("a", Some("2.3.0")));

    track(&tracker, &mut [error("a", "2.3.0", NOW)], NOW).await;

    assert_eq!(issues.kept.lock()["a"].status, RumIssueStatus::Resolved);
}

#[tokio::test]
async fn a_resolved_issue_of_an_application_without_versions_regresses_by_time() {
    let (tracker, issues) = tracker(100, &[]);

    issues.kept.lock().insert("a".into(), resolved("a", None));

    track(&tracker, &mut [error("a", "", NOW)], NOW).await;

    assert_eq!(issues.kept.lock()["a"].status, RumIssueStatus::Regressed);
}

#[tokio::test]
async fn an_issue_that_keeps_happening_is_read_again_when_what_is_known_of_it_expires() {
    let (tracker, issues) = tracker(100, &[("2.3.0", 100), ("2.4.1", 200)]);

    track(&tracker, &mut [error("a", "2.3.0", NOW)], NOW).await;

    // A person resolves it while it keeps happening, in a version that is the newest.
    {
        let mut kept = issues.kept.lock();
        let issue = kept.get_mut("a").unwrap();

        issue.status = RumIssueStatus::Resolved;
        issue.resolved_at = Some(TimestampMicros(NOW + 1));
        issue.resolved_version = Some("2.3.0".into());
    }

    // Writing a sighting of it every minute does not keep the old state alive.
    track(&tracker, &mut [error("a", "2.3.0", NOW + MINUTE)], NOW).await;
    track(&tracker, &mut [error("a", "2.3.0", NOW + 2 * MINUTE)], NOW).await;

    // What is known of it runs out; it is read again, and the newer version reopens it.
    tracker.known.invalidate(&(org(), "a".to_string()));
    track(&tracker, &mut [error("a", "2.4.1", NOW + 3 * MINUTE)], NOW).await;

    let kept = issues.kept.lock()["a"].clone();

    assert_eq!(kept.status, RumIssueStatus::Regressed);
    assert_eq!(kept.regressed_version.as_deref(), Some("2.4.1"));
}

#[tokio::test]
async fn an_error_in_a_new_version_reads_the_issue_again_before_it_is_judged() {
    let (tracker, issues) = tracker(100, &[("2.3.0", 100), ("2.4.1", 200)]);

    track(&tracker, &mut [error("a", "2.3.0", NOW)], NOW).await;

    // A person resolves it on another node: this one still holds it as open.
    {
        let mut kept = issues.kept.lock();
        let issue = kept.get_mut("a").unwrap();

        issue.status = RumIssueStatus::Resolved;
        issue.resolved_at = Some(TimestampMicros(NOW + 1));
        issue.resolved_version = Some("2.3.0".into());
    }

    assert_eq!(
        tracker.known.get(&(org(), "a".to_string())).unwrap().status,
        RumIssueStatus::Open
    );

    // Within the minute it is known for, the first error of a newer version brings it back.
    track(&tracker, &mut [error("a", "2.4.1", NOW + 1_000)], NOW).await;

    let kept = issues.kept.lock()["a"].clone();

    assert_eq!(kept.status, RumIssueStatus::Regressed);
    assert_eq!(kept.regressed_version.as_deref(), Some("2.4.1"));
}

#[tokio::test]
async fn an_issue_is_read_again_once_for_a_version_and_not_for_each_of_its_errors() {
    let (tracker, issues) = tracker(100, &[]);

    track(&tracker, &mut [error("a", "2.3.0", NOW)], NOW).await;
    assert_eq!(*issues.reads.lock(), 1);

    // The version it was opened in is not new.
    track(&tracker, &mut [error("a", "2.3.0", NOW + 1_000)], NOW).await;
    assert_eq!(*issues.reads.lock(), 1);

    track(&tracker, &mut [error("a", "2.4.1", NOW + 2_000)], NOW).await;
    assert_eq!(*issues.reads.lock(), 2);

    // Nor is the one that was read for, however many errors it brings.
    let mut more = vec![
        error("a", "2.4.1", NOW + 3_000),
        error("a", "2.4.1", NOW + 4_000),
    ];

    track(&tracker, &mut more, NOW).await;
    assert_eq!(*issues.reads.lock(), 2);

    track(&tracker, &mut [error("a", "2.5.0", NOW + 5_000)], NOW).await;
    assert_eq!(*issues.reads.lock(), 3);
}

#[tokio::test]
async fn an_issue_that_cannot_be_read_again_is_judged_as_it_was_known() {
    let (tracker, issues) = tracker(100, &[]);

    track(&tracker, &mut [error("a", "2.3.0", NOW)], NOW).await;

    *issues.unreadable.lock() = true;

    track(&tracker, &mut [error("a", "2.4.1", NOW + 1_000)], NOW).await;

    // The newer version was still written as seen.
    let written = issues.sightings.lock().clone();

    assert_eq!(written.len(), 1);
    assert_eq!(written[0][0].version.as_deref(), Some("2.4.1"));
}

#[tokio::test]
async fn an_issue_that_cannot_be_read_leaves_the_rows_as_they_came() {
    let (tracker, issues) = tracker(1, &[]);

    *issues.unreadable.lock() = true;

    let mut rows = vec![error("a", "1", NOW), error("b", "1", NOW)];

    tracker.track(&org(), &mut rows).await;

    assert_eq!(
        rows.iter().map(issue_of).collect::<Vec<_>>(),
        vec!["a".to_string(), "b".to_string()]
    );
    assert!(issues.opened.lock().is_empty());
}

#[tokio::test]
async fn a_row_without_an_issue_or_an_application_is_left_alone() {
    let (tracker, issues) = tracker(100, &[]);

    let mut bare = error("a", "1", NOW);

    bare.fields.remove("error.issue_id");

    let mut anonymous = error("b", "1", NOW);

    anonymous.fields.remove("application.id");

    let mut rows = vec![bare, anonymous];

    track(&tracker, &mut rows, NOW).await;

    assert!(issues.opened.lock().is_empty());
}

#[tokio::test]
async fn the_first_sight_of_an_issue_in_each_environment_is_written_once() {
    let (tracker, issues) = tracker(100, &[]);

    let mut first = vec![
        error_in("a", "2.4.1", "staging", NOW),
        error_in("a", "2.4.1", "prod", NOW + 1_000),
        error_in("a", "2.4.1", "prod", NOW + 500),
    ];

    track(&tracker, &mut first, NOW).await;

    let mut later = vec![
        error_in("a", "2.4.1", "prod", NOW + 2_000),
        error_in("a", "2.4.1", "canary", NOW + 3_000),
    ];

    track(&tracker, &mut later, NOW).await;

    assert_eq!(
        *issues.environments.lock(),
        vec![
            vec![
                ("a".to_string(), "staging".to_string(), NOW),
                // The earliest of the errors in the batch is the first sight.
                ("a".to_string(), "prod".to_string(), NOW + 500),
            ],
            vec![("a".to_string(), "canary".to_string(), NOW + 3_000)],
        ]
    );
}

#[tokio::test]
async fn an_error_without_an_environment_leaves_none_behind() {
    let (tracker, issues) = tracker(100, &[]);

    track(&tracker, &mut [error("a", "2.4.1", NOW)], NOW).await;

    assert!(issues.environments.lock().is_empty());
}

#[tokio::test]
async fn a_regression_remembers_where_the_issue_came_back() {
    let (tracker, issues) = tracker(100, &[("2.3.0", 100), ("2.4.1", 200)]);

    issues
        .kept
        .lock()
        .insert("a".into(), resolved("a", Some("2.3.0")));

    // Seen again in the version it was resolved in, in staging; then in a newer one, in prod.
    let mut rows = vec![
        error_in("a", "2.3.0", "staging", NOW),
        error_in("a", "2.4.1", "prod", NOW + 1_000),
    ];

    track(&tracker, &mut rows, NOW).await;

    let kept = issues.kept.lock()["a"].clone();

    assert_eq!(kept.status, RumIssueStatus::Regressed);
    assert_eq!(kept.regressed_environment.as_deref(), Some("prod"));
}

#[tokio::test]
async fn what_is_counted_as_the_rest_keeps_the_environments_of_its_errors() {
    let (tracker, issues) = tracker(1, &[]);

    let mut rows = vec![
        error_in("a", "1", "prod", NOW),
        error_in("b", "1", "prod", NOW),
        error_in("c", "1", "staging", NOW),
    ];

    track(&tracker, &mut rows, NOW).await;

    let rest = overflow_id("shop-web");
    let written = issues
        .environments
        .lock()
        .iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();

    assert!(written.contains(&("a".to_string(), "prod".to_string(), NOW)));
    assert!(written.contains(&(rest.clone(), "prod".to_string(), NOW)));
    assert!(written.contains(&(rest, "staging".to_string(), NOW)));
    assert_eq!(written.len(), 3);
}
