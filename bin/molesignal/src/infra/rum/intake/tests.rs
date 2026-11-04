// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashMap, sync::LazyLock};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{Value, json};

use super::{sample::keeps, *};
use crate::{
    domain::{
        intake::{IntakeError, IntakeResult},
        rum::{
            ERRORS_FILTER, RetentionCondition, RetentionFilter, RetentionOp, RumApplicationPatch,
            RumApplicationSettings, RumPlatform, normalize_filters,
        },
        stream::{Schema, StreamDefinition},
    },
    infra::enrichment::GeoLocation,
};

const NOW: i64 = 1_760_000_000_000;

static ORG: LazyLock<Id> = LazyLock::new(|| Id::from_string("org-1"));

#[derive(Default)]
struct RecordingWriter {
    batches: Mutex<Vec<IntakeBatch>>,
    /// `(stream, index in the batch)` the intake refuses.
    refuse: Mutex<Vec<(String, usize)>>,
    fail: bool,
}

#[async_trait]
impl RumEventWriter for RecordingWriter {
    async fn write(&self, batch: IntakeBatch) -> Result<IntakeResult> {
        if self.fail {
            return Err(Error::unavailable("storage is down"));
        }

        let errors = self
            .refuse
            .lock()
            .iter()
            .filter(|(stream, _)| stream == &batch.stream)
            .map(|(_, index)| IntakeError {
                index: *index,
                reason: "type mismatch".into(),
            })
            .collect::<Vec<_>>();

        let result = IntakeResult {
            accepted: batch.events.len() - errors.len(),
            rejected: errors.len(),
            errors,
        };

        self.batches.lock().push(batch);
        Ok(result)
    }
}

#[derive(Default)]
struct Streams;

#[async_trait]
impl StreamRepository for Streams {
    async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
        Ok(def)
    }

    async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
        Ok(())
    }

    async fn get(&self, _org: &Id, _name: &str, _kind: StreamType) -> Result<StreamDefinition> {
        Err(Error::not_found("stream"))
    }

    async fn list(&self, _org: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(Vec::new())
    }

    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct Applications {
    by_id: Mutex<HashMap<String, RumApplication>>,
}

impl Applications {
    fn with(settings: RumApplicationSettings) -> Self {
        let applications = Self::default();

        applications.by_id.lock().insert(
            "shop-web".into(),
            RumApplication {
                org_id: org(),
                id: "shop-web".into(),
                name: "Shop".into(),
                platform: RumPlatform::Browser,
                settings,
                created_by: None,
                created_at: TimestampMicros(0),
                updated_at: TimestampMicros(0),
            },
        );

        applications
    }
}

#[async_trait]
impl RumApplicationRepository for Applications {
    async fn create(&self, application: RumApplication) -> Result<RumApplication> {
        Ok(application)
    }

    async fn get(&self, _org_id: &Id, id: &str) -> Result<RumApplication> {
        self.by_id
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| Error::not_found("application"))
    }

    async fn list(&self, _org_id: &Id) -> Result<Vec<RumApplication>> {
        Ok(self.by_id.lock().values().cloned().collect())
    }

    async fn update(
        &self,
        _org_id: &Id,
        id: &str,
        _patch: RumApplicationPatch,
    ) -> Result<RumApplication> {
        self.get(&org(), id).await
    }

    async fn delete(&self, _org_id: &Id, _id: &str) -> Result<()> {
        Ok(())
    }
}

struct Geo;

impl GeoLookup for Geo {
    fn lookup(&self, ip: IpAddr) -> Option<GeoLocation> {
        (ip.to_string() == "203.0.113.7").then(|| GeoLocation {
            country: Some("DE".into()),
            ..GeoLocation::default()
        })
    }
}

fn org() -> Id {
    ORG.clone()
}

fn intake_with(
    writer: Arc<RecordingWriter>,
    settings: RumApplicationSettings,
) -> (RumIntake, Arc<RecordingWriter>) {
    let intake = RumIntake::new(
        writer.clone(),
        Arc::new(Streams),
        Arc::new(Applications::with(settings)),
        Arc::new(Geo),
    );

    (intake, writer)
}

fn intake() -> (RumIntake, Arc<RecordingWriter>) {
    intake_with(Arc::default(), RumApplicationSettings::default())
}

fn view(id: &str, revision: i64) -> Value {
    json!({
        "type": "view", "date": NOW, "source": "browser", "service": "web",
        "session": {"id": "s-1"},
        "view": {"id": id, "url": "https://shop.example.com/users/42", "revision": revision, "time_spent": 5}
    })
}

fn resource(url: &str) -> Value {
    json!({
        "type": "resource", "date": NOW + 10, "source": "browser",
        "session": {"id": "s-1"}, "view": {"id": "v-1"},
        "resource": {"type": "fetch", "url": url, "status_code": 200, "duration": 1_000_000}
    })
}

fn request(
    events: Vec<Value>,
    bound: Option<&'static str>,
    intake: &RumIntake,
) -> Request<'static> {
    Request {
        org_id: &ORG,
        bound_application: bound,
        events: events.into_iter().map(Ok).collect(),
        origin: intake.origin(
            Some("203.0.113.7".parse().unwrap()),
            Some("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36"),
        ),
    }
}

fn batch(writer: &RecordingWriter, stream: &str) -> Option<IntakeBatch> {
    writer
        .batches
        .lock()
        .iter()
        .find(|batch| batch.stream == stream)
        .cloned()
}

#[tokio::test]
async fn events_are_written_to_the_stream_of_their_type_as_flat_rows() {
    let (intake, writer) = intake();

    let report = intake
        .intake(request(
            vec![
                view("v-1", 1),
                resource("https://api.example.com/carts/12345"),
            ],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    assert_eq!(
        report,
        Report {
            accepted: 2,
            rejected: vec![],
            sampled: 0,
        }
    );

    let views = batch(&writer, "rum_views").expect("views written");
    let row = &views.events[0];

    assert_eq!(row.timestamp, TimestampMicros(NOW * 1000));
    assert_eq!(row.fields["application.id"], "shop-web");
    assert_eq!(row.fields["session.id"], "s-1");
    assert_eq!(row.fields["view.revision"], 1);
    assert_eq!(row.fields["view.url_path_group"], "/users/?");
    assert_eq!(row.fields["geo.country_iso_code"], "DE");
    assert_eq!(row.fields["network.client.ip"], "203.0.113.7");
    assert_eq!(row.fields["browser.name"], "Chrome");
    assert!(!row.fields.contains_key("type"));
    assert!(!row.fields.contains_key("date"));

    let resources = batch(&writer, "rum_resources").expect("resources written");

    assert_eq!(
        resources.events[0].fields["resource.url_path_group"],
        "/carts/?"
    );
    assert_eq!(resources.stream_type, StreamType::LOGS);
}

#[tokio::test]
async fn every_view_is_also_sent_to_sessions_as_a_patch() {
    let (intake, writer) = intake();

    intake
        .intake(request(
            vec![view("v-1", 1), view("v-1", 2)],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    let sessions = batch(&writer, "rum_sessions").expect("a patch");
    let views = batch(&writer, "rum_views").expect("a view");

    assert_eq!(sessions.events.len(), 1);
    assert_eq!(views.events.len(), 1);
    assert_eq!(sessions.events[0].fields["view.revision"], 2);
    assert_eq!(views.events[0].fields["view.revision"], 2);
}

#[tokio::test]
async fn a_bad_event_is_refused_alone() {
    let (intake, writer) = intake();
    let bad = json!({"type": "view", "date": NOW});

    let report = intake
        .intake(request(
            vec![
                resource("https://a.example/x"),
                bad,
                resource("https://a.example/y"),
            ],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    assert_eq!(report.accepted, 2);
    assert_eq!(report.rejected.len(), 1);
    assert_eq!(report.rejected[0].index, 1);
    assert_eq!(batch(&writer, "rum_resources").unwrap().events.len(), 2);
}

#[tokio::test]
async fn a_line_that_was_not_json_is_refused_by_its_place() {
    let (intake, _writer) = intake();

    let mut ask = request(
        vec![resource("https://a.example/x")],
        Some("shop-web"),
        &intake,
    );

    ask.events.insert(0, Err("expected value at line 1".into()));

    let report = intake.intake(ask).await.unwrap();

    assert_eq!(report.accepted, 1);
    assert_eq!(report.rejected[0].index, 0);
    assert!(report.rejected[0].reason.contains("not JSON"));
}

#[tokio::test]
async fn an_event_cannot_name_another_application_than_its_token() {
    let (intake, writer) = intake();
    let mut other = resource("https://a.example/x");

    other["application"] = json!({"id": "other-app"});

    let report = intake
        .intake(request(vec![other], Some("shop-web"), &intake))
        .await
        .unwrap();

    assert_eq!(report.accepted, 0);
    assert!(report.rejected[0].reason.contains("client token"));
    assert!(writer.batches.lock().is_empty());
}

#[tokio::test]
async fn an_unknown_application_is_refused() {
    let (intake, writer) = intake();

    let report = intake
        .intake(request(
            vec![resource("https://a.example/x")],
            Some("ghost"),
            &intake,
        ))
        .await
        .unwrap();

    assert_eq!(report.accepted, 0);
    assert!(report.rejected[0].reason.contains("does not exist"));
    assert!(writer.batches.lock().is_empty());
}

#[tokio::test]
async fn a_credential_bound_to_nothing_leaves_each_event_to_name_its_application() {
    let (intake, writer) = intake();
    let mut named = resource("https://a.example/x");

    named["application"] = json!({"id": "shop-web"});

    let report = intake
        .intake(request(
            vec![named, resource("https://a.example/y")],
            None,
            &intake,
        ))
        .await
        .unwrap();

    assert_eq!(report.accepted, 1);
    assert!(report.rejected[0].reason.contains("required"));
    assert_eq!(batch(&writer, "rum_resources").unwrap().events.len(), 1);
}

#[tokio::test]
async fn the_application_decides_what_is_kept_of_the_client() {
    let settings = RumApplicationSettings {
        collect_ip: false,
        ..RumApplicationSettings::default()
    };
    let (intake, writer) = intake_with(Arc::default(), settings);

    intake
        .intake(request(
            vec![resource("https://a.example/x")],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    let row = &batch(&writer, "rum_resources").unwrap().events[0];

    assert!(!row.fields.contains_key("network.client.ip"));
    assert_eq!(row.fields["geo.country_iso_code"], "DE");
}

#[tokio::test]
async fn a_row_the_intake_refuses_is_named_by_its_place_in_the_request() {
    let writer = Arc::new(RecordingWriter::default());

    writer.refuse.lock().push(("rum_resources".into(), 1));

    let (intake, writer) = intake_with(writer, RumApplicationSettings::default());

    let report = intake
        .intake(request(
            vec![
                view("v-1", 1),
                resource("https://a.example/x"),
                view("v-2", 1),
                resource("https://a.example/y"),
            ],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    assert_eq!(report.accepted, 3);
    assert_eq!(report.rejected.len(), 1);
    assert_eq!(report.rejected[0].index, 3);
    assert!(report.rejected[0].reason.starts_with("rum_resources"));
    assert_eq!(writer.batches.lock().len(), 3);
}

#[tokio::test]
async fn a_failing_store_fails_the_request() {
    let writer = Arc::new(RecordingWriter {
        fail: true,
        ..RecordingWriter::default()
    });
    let (intake, _writer) = intake_with(writer, RumApplicationSettings::default());

    let error = intake
        .intake(request(
            vec![resource("https://a.example/x")],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap_err();

    assert!(matches!(error, Error::Unavailable(_)));
}

#[tokio::test]
async fn a_request_of_only_bad_events_writes_nothing() {
    let (intake, writer) = intake();

    let report = intake
        .intake(request(
            vec![json!({"nope": true})],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    assert_eq!(report.accepted, 0);
    assert_eq!(report.rejected.len(), 1);
    assert!(writer.batches.lock().is_empty());
}

fn error(message: &str, extra: Value) -> Value {
    let mut event = json!({
        "type": "error", "date": NOW + 20, "source": "browser", "version": "2.4.1",
        "session": {"id": "s-1"}, "view": {"id": "v-1"},
        "error": {"id": "e-1", "message": message, "source": "source", "type": "TypeError"}
    });

    if let Value::Object(extra) = extra {
        event["error"].as_object_mut().unwrap().extend(extra);
    }

    event
}

#[tokio::test]
async fn an_error_row_carries_the_id_of_its_issue_and_other_rows_do_not() {
    let (intake, writer) = intake();

    intake
        .intake(request(
            vec![
                error("Loading chunk 482 failed.", json!({})),
                error("Loading chunk 17 failed.", json!({})),
                error("Loading chunk 17 failed.", json!({"fingerprint": "chunks"})),
                resource("https://a.example/x"),
            ],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    let errors = batch(&writer, "rum_errors").expect("errors written");
    let ids = errors
        .events
        .iter()
        .map(|row| row.fields["error.issue_id"].as_str().unwrap().to_string())
        .collect::<Vec<_>>();

    // The numbers of a message do not tell issues apart; a fingerprint does.
    assert_eq!(ids[0], ids[1]);
    assert_ne!(ids[0], ids[2]);
    assert_eq!(ids[0].len(), 16);

    let resources = batch(&writer, "rum_resources").unwrap();

    assert!(!resources.events[0].fields.contains_key("error.issue_id"));
}

#[tokio::test]
async fn an_issue_id_an_sdk_sends_is_not_taken() {
    let (intake, writer) = intake();

    intake
        .intake(request(
            vec![error("boom", json!({"issue_id": "mine"}))],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    let errors = batch(&writer, "rum_errors").unwrap();

    assert_ne!(errors.events[0].fields["error.issue_id"], "mine");
}

#[derive(Default)]
struct Versions {
    noted: Mutex<Vec<(String, Vec<String>)>>,
}

#[async_trait]
impl RumVersionRepository for Versions {
    async fn record(
        &self,
        _org_id: &Id,
        application_id: &str,
        versions: &[String],
        _at: TimestampMicros,
    ) -> Result<()> {
        self.noted
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
async fn the_versions_events_come_with_are_noted_once_for_their_application() {
    let versions = Arc::new(Versions::default());
    let (intake, _writer) = intake();
    let intake = intake.with_versions(versions.clone());

    for _ in 0..2 {
        intake
            .intake(request(
                vec![error("boom", json!({})), error("bang", json!({}))],
                Some("shop-web"),
                &intake,
            ))
            .await
            .unwrap();
    }

    assert_eq!(
        *versions.noted.lock(),
        vec![("shop-web".to_string(), vec!["2.4.1".to_string()])]
    );
}

fn of_session(session: &str, mut event: Value) -> Value {
    event["session"] = json!({ "id": session });

    event
}

fn sessions(count: usize) -> Vec<String> {
    (0..count)
        .map(|n| format!("6f1c{n:08x}-1b7e-4d08-a5c3-90e2d41f7b68"))
        .collect()
}

/// An application that keeps this share of its sessions, errors and all.
fn with_sample_rate(rate: u32) -> RumApplicationSettings {
    RumApplicationSettings {
        session_sample_rate: rate,
        keep_errors: false,
        ..RumApplicationSettings::default()
    }
}

#[tokio::test]
async fn a_session_the_application_does_not_keep_leaves_nothing_behind_and_the_answer_counts_what_was_left_out()
 {
    let (intake, writer) = intake_with(Arc::default(), with_sample_rate(40));
    let ids = sessions(200);

    let events = ids
        .iter()
        .flat_map(|id| {
            [
                of_session(id, view(&format!("view-of-{id}"), 1)),
                of_session(id, resource("https://a.example/x")),
                of_session(id, error("boom", json!({}))),
            ]
        })
        .collect::<Vec<_>>();

    let report = intake
        .intake(request(events, Some("shop-web"), &intake))
        .await
        .unwrap();

    let kept = ids.iter().filter(|id| keeps(id, 40)).count();

    assert!(kept > 50 && kept < 110, "kept {kept} of 200");
    assert_eq!(report.accepted, kept * 3);
    assert_eq!(report.sampled, (200 - kept) * 3);
    assert!(report.rejected.is_empty());

    // Every stream holds the sessions that are kept, each with all it did, and no other.
    for stream in ["rum_views", "rum_resources", "rum_errors", "rum_sessions"] {
        let written = batch(&writer, stream).expect(stream);

        assert_eq!(written.events.len(), kept, "{stream}");
        assert!(
            written
                .events
                .iter()
                .all(|row| keeps(row.fields["session.id"].as_str().unwrap(), 40)),
            "{stream}"
        );
    }
}

#[tokio::test]
async fn an_application_that_keeps_no_session_writes_nothing() {
    let (intake, writer) = intake_with(Arc::default(), with_sample_rate(0));

    let report = intake
        .intake(request(
            vec![view("v-1", 1), resource("https://a.example/x")],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    assert_eq!(report.accepted, 0);
    assert_eq!(report.sampled, 2);
    assert!(writer.batches.lock().is_empty());

    // An event that is refused is still refused, not left out.
    let bad = json!({"type": "view", "date": NOW});
    let report = intake
        .intake(request(vec![bad], Some("shop-web"), &intake))
        .await
        .unwrap();

    assert_eq!(report.sampled, 0);
    assert_eq!(report.rejected.len(), 1);
}

#[test]
fn the_answer_says_how_many_events_were_left_out_only_when_some_were() {
    let none = serde_json::to_value(Report {
        accepted: 3,
        ..Report::default()
    })
    .unwrap();

    assert_eq!(none, json!({ "accepted": 3, "rejected": [] }));

    let some = serde_json::to_value(Report {
        accepted: 3,
        sampled: 5,
        ..Report::default()
    })
    .unwrap();

    assert_eq!(some, json!({ "accepted": 3, "rejected": [], "sampled": 5 }));
}

#[tokio::test]
async fn sensitive_text_is_taken_out_before_anything_is_written() {
    let (intake, writer) = intake();

    let mut leaking = view("v-1", 1);

    leaking["view"]["url"] =
        json!("https://shop.example.com/users/jane@example.com?token=abc123&page=2");
    leaking["view"]["referrer"] = json!("https://mail.example.com/?code=zzz");

    let mut failing = resource("https://api.example.com/pay?access_token=abc");

    failing["resource"]["request"] = json!({ "headers": { "Authorization": "Bearer abcdefghijklmnop1234", "Accept": "application/json" } });

    intake
        .intake(request(
            vec![
                leaking,
                failing,
                error(
                    "Cannot charge jane@example.com: card 4111 1111 1111 1111",
                    json!({}),
                ),
            ],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    let views = batch(&writer, "rum_views").unwrap();
    let row = &views.events[0].fields;

    assert_eq!(
        row["view.url"],
        "https://shop.example.com/users/[email]?token=[redacted]&page=2"
    );
    assert_eq!(
        row["view.referrer"],
        "https://mail.example.com/?code=[redacted]"
    );
    // What is derived from the address is derived from what is kept of it.
    assert_eq!(row["view.url_path"], "/users/[email]");
    assert_eq!(row["view.url_host"], "shop.example.com");

    let resources = batch(&writer, "rum_resources").unwrap();
    let resource = &resources.events[0].fields;

    assert_eq!(
        resource["resource.url"],
        "https://api.example.com/pay?access_token=[redacted]"
    );

    let errors = batch(&writer, "rum_errors").unwrap();

    assert_eq!(
        errors.events[0].fields["error.message"],
        "Cannot charge [email]: card [card]"
    );
}

#[tokio::test]
async fn errors_that_differ_only_in_what_was_taken_out_are_one_issue() {
    let (intake, writer) = intake();

    intake
        .intake(request(
            vec![
                error("Cannot charge jane@example.com", json!({})),
                error("Cannot charge bob@example.org", json!({})),
            ],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    let errors = batch(&writer, "rum_errors").unwrap();

    assert_eq!(
        errors.events[0].fields["error.issue_id"],
        errors.events[1].fields["error.issue_id"]
    );
}

#[tokio::test]
async fn an_application_that_scrubs_nothing_keeps_the_text_as_it_was_sent() {
    let settings = RumApplicationSettings {
        scrub: Vec::new(),
        ..RumApplicationSettings::default()
    };
    let (intake, writer) = intake_with(Arc::default(), settings);

    intake
        .intake(request(
            vec![error("Cannot charge jane@example.com", json!({}))],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    let errors = batch(&writer, "rum_errors").unwrap();

    assert_eq!(
        errors.events[0].fields["error.message"],
        "Cannot charge jane@example.com"
    );
}

fn retention(
    rest: u32,
    filters: Vec<RetentionFilter>,
    keep_errors: bool,
) -> RumApplicationSettings {
    RumApplicationSettings {
        session_sample_rate: rest,
        retention_filters: normalize_filters(filters).unwrap(),
        keep_errors,
        ..RumApplicationSettings::default()
    }
}

fn filter(name: &str, rate: u32, field: &str, values: &[&str]) -> RetentionFilter {
    RetentionFilter {
        id: String::new(),
        name: name.into(),
        rate,
        conditions: vec![RetentionCondition {
            field: field.into(),
            op: RetentionOp::Is,
            values: values.iter().map(|value| value.to_string()).collect(),
        }],
    }
}

/// An event of a session that was run in an environment.
fn in_env(session: &str, env: &str, mut event: Value) -> Value {
    event["session"] = json!({ "id": session });
    event["env"] = json!(env);

    event
}

#[tokio::test]
async fn every_row_says_the_share_of_such_rows_that_is_kept_and_what_kept_it() {
    let (intake, writer) = intake();

    intake
        .intake(request(
            vec![view("v-1", 1), resource("https://a.example/x")],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    for stream in ["rum_views", "rum_sessions", "rum_resources"] {
        let row = &batch(&writer, stream).expect(stream).events[0];

        assert_eq!(row.fields["retention.rate"], json!(100.0), "{stream}");
        assert_eq!(row.fields["retention.filter"], "default", "{stream}");
    }
}

#[tokio::test]
async fn what_an_sdk_says_of_how_its_event_was_kept_is_not_believed() {
    let (intake, writer) = intake();

    let mut forged = resource("https://a.example/x");

    forged["retention"] = json!({ "rate": 1, "filter": "forged" });

    intake
        .intake(request(vec![forged], Some("shop-web"), &intake))
        .await
        .unwrap();

    let row = &batch(&writer, "rum_resources").unwrap().events[0];

    assert_eq!(row.fields["retention.rate"], json!(100.0));
    assert_eq!(row.fields["retention.filter"], "default");
}

#[tokio::test]
async fn a_filter_keeps_the_sessions_it_describes_by_its_own_share() {
    let settings = retention(
        0,
        vec![
            filter("Production", 100, "env", &["prod"]),
            filter("Beta", 30, "env", &["beta"]),
        ],
        false,
    );
    let (intake, writer) = intake_with(Arc::default(), settings);
    let ids = sessions(300);

    let events = ids
        .iter()
        .enumerate()
        .flat_map(|(n, id)| {
            let env = ["prod", "beta", "dev"][n % 3];

            [
                in_env(id, env, view(&format!("v-{n}"), 1)),
                in_env(id, env, resource("https://a.example/x")),
            ]
        })
        .collect::<Vec<_>>();

    let report = intake
        .intake(request(events, Some("shop-web"), &intake))
        .await
        .unwrap();

    let prod = ids.iter().enumerate().filter(|(n, _)| n % 3 == 0).count();
    let beta = ids
        .iter()
        .enumerate()
        .filter(|(n, id)| n % 3 == 1 && keeps(id, 30))
        .count();

    // Production is all kept, beta by its share, and development, which no filter describes, by the
    // share left for the rest, which is none.
    assert_eq!(report.accepted, (prod + beta) * 2);
    assert_eq!(report.sampled, (300 - prod - beta) * 2);

    let rows = batch(&writer, "rum_resources").unwrap().events;

    assert_eq!(rows.len(), prod + beta);

    for row in rows {
        let (filter, rate) = match row.fields["env"].as_str().unwrap() {
            "prod" => ("production", 100.0),
            "beta" => ("beta", 30.0),
            other => panic!("a session of {other} was kept"),
        };

        assert_eq!(row.fields["retention.filter"], filter);
        assert_eq!(row.fields["retention.rate"], json!(rate));
    }
}

#[tokio::test]
async fn the_errors_of_sessions_that_are_not_kept_are_kept_alone_with_the_whole_share() {
    let (intake, writer) = intake_with(Arc::default(), retention(0, Vec::new(), true));
    let ids = sessions(40);

    let events = ids
        .iter()
        .flat_map(|id| {
            [
                of_session(id, view("v-1", 1)),
                of_session(id, resource("https://a.example/x")),
                of_session(id, error("boom", json!({}))),
            ]
        })
        .collect::<Vec<_>>();

    let report = intake
        .intake(request(events, Some("shop-web"), &intake))
        .await
        .unwrap();

    assert_eq!(report.accepted, 40);
    assert_eq!(report.sampled, 80);

    let errors = batch(&writer, "rum_errors").unwrap().events;

    assert_eq!(errors.len(), 40);

    for row in &errors {
        assert_eq!(row.fields["retention.rate"], json!(100.0));
        assert_eq!(row.fields["retention.filter"], ERRORS_FILTER);
        // The row is complete: the issue it belongs to is worked out like any other's.
        assert!(row.fields.contains_key("error.issue_id"));
    }

    // Nothing else of those sessions is written, and no session is made of the errors.
    for stream in ["rum_views", "rum_resources", "rum_sessions"] {
        assert!(batch(&writer, stream).is_none(), "{stream}");
    }
}

#[tokio::test]
async fn with_errors_kept_every_error_has_the_whole_share_and_the_rest_has_the_share_of_its_session()
 {
    let (intake, writer) = intake_with(Arc::default(), retention(50, Vec::new(), true));
    let ids = sessions(200);

    // Every session has a view of its own: one id would be one view to the intake.
    let events = ids
        .iter()
        .enumerate()
        .flat_map(|(n, id)| {
            [
                of_session(id, view(&format!("v-{n}"), 1)),
                of_session(id, error("boom", json!({}))),
            ]
        })
        .collect::<Vec<_>>();

    intake
        .intake(request(events, Some("shop-web"), &intake))
        .await
        .unwrap();

    let kept = ids.iter().filter(|id| keeps(id, 50)).count();
    let errors = batch(&writer, "rum_errors").unwrap().events;
    let views = batch(&writer, "rum_views").unwrap().events;

    assert_eq!(errors.len(), 200);
    assert_eq!(views.len(), kept);
    assert!(
        errors
            .iter()
            .all(|row| row.fields["retention.rate"] == json!(100.0))
    );
    assert!(
        views
            .iter()
            .all(|row| row.fields["retention.rate"] == json!(50.0))
    );

    // An error of a session that is kept says which filter kept the session, and the others
    // say they are errors.
    let by = |filter: &str| {
        errors
            .iter()
            .filter(|row| row.fields["retention.filter"] == filter)
            .count()
    };

    assert_eq!(by("default"), kept);
    assert_eq!(by(ERRORS_FILTER), 200 - kept);
}

#[tokio::test]
async fn without_errors_kept_an_error_goes_with_its_session() {
    let (intake, writer) = intake_with(Arc::default(), retention(0, Vec::new(), false));

    let report = intake
        .intake(request(
            vec![error("boom", json!({})), view("v-1", 1)],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    assert_eq!(report.accepted, 0);
    assert_eq!(report.sampled, 2);
    assert!(writer.batches.lock().is_empty());
}

#[tokio::test]
async fn a_replay_is_kept_when_a_filter_that_might_describe_its_session_would_keep_it() {
    let settings = retention(0, vec![filter("Production", 100, "env", &["prod"])], false);
    let (intake, _writer) = intake_with(Arc::default(), settings.clone());
    let origin = intake.origin(None, None);

    let said = |env: Option<&str>| {
        let mut said = Map::new();

        if let Some(env) = env {
            said.insert("env".into(), json!(env));
        }

        said
    };

    // The environment is not said: any session might be a production one.
    assert!(intake.keeps_replay(&settings, "s-1", said(None), &origin));
    assert!(intake.keeps_replay(&settings, "s-2", said(Some("prod")), &origin));
    // It is said, and nothing keeps development.
    assert!(!intake.keeps_replay(&settings, "s-1", said(Some("dev")), &origin));
    assert!(!intake.keeps_replay(&settings, "s-2", said(Some("dev")), &origin));
}

#[tokio::test]
async fn a_log_goes_to_its_own_stream_with_what_it_says_and_is_not_counted_as_a_view() {
    let (intake, writer) = intake();

    let log = json!({
        "type": "log",
        "date": NOW,
        "source": "browser",
        "service": "web",
        "session": { "id": "6f1c2b7e-1d5a-4c6b-9a3e-0a1b2c3d4e5f", "type": "user" },
        "view": { "id": "view-1", "url": "https://shop.example/cart" },
        "level": "warn",
        "message": "cart is stale",
        "log": {
            "id": "log-1",
            "logger": "cart",
            "origin": "logger",
            "error_type": "StaleCart",
            "stack": "at cart.ts:1",
        },
    });

    let report = intake
        .intake(request(vec![log], Some("shop-web"), &intake))
        .await
        .unwrap();

    assert_eq!(report.accepted, 1);

    let batch = batch(&writer, "rum_logs").expect("a batch of logs");
    let row = &batch.events[0].fields;

    assert_eq!(row["level"], "warn");
    assert_eq!(row["message"], "cart is stale");
    assert_eq!(row["log.id"], "log-1");
    assert_eq!(row["log.logger"], "cart");
    assert_eq!(row["log.origin"], "logger");
    assert_eq!(row["log.error_type"], "StaleCart");
    assert_eq!(row["session.id"], "6f1c2b7e-1d5a-4c6b-9a3e-0a1b2c3d4e5f");
    assert_eq!(row["view.url_path"], "/cart");
}

#[tokio::test]
async fn what_a_log_says_is_scrubbed_like_any_other_text() {
    let (intake, writer) = intake();

    let log = json!({
        "type": "log",
        "date": NOW,
        "source": "browser",
        "session": { "id": "6f1c2b7e-1d5a-4c6b-9a3e-0a1b2c3d4e5f" },
        "view": { "id": "view-1", "url": "https://shop.example/cart" },
        "level": "error",
        "message": "mail ada@example.com failed",
        "log": { "id": "log-2", "stack": "at send(ada@example.com)" },
    });

    intake
        .intake(request(vec![log], Some("shop-web"), &intake))
        .await
        .unwrap();

    let batch = batch(&writer, "rum_logs").expect("a batch of logs");
    let row = &batch.events[0].fields;

    assert!(
        !row["message"].as_str().unwrap().contains("ada@example.com"),
        "{row:?}"
    );
    assert!(
        !row["log.stack"]
            .as_str()
            .unwrap()
            .contains("ada@example.com"),
        "{row:?}"
    );
}

fn driven_by_a_check(session: &str, mut event: Value) -> Value {
    event["session"] = json!({ "id": session, "type": "synthetics" });
    event["synthetics"] =
        json!({ "test_id": "check-1", "result_id": "run-1", "location_id": "eu" });

    event
}

#[tokio::test]
async fn a_session_a_test_drives_is_kept_whole_and_its_errors_open_no_issue() {
    let (intake, writer) = intake_with(Arc::default(), with_sample_rate(0));
    let id = sessions(1).remove(0);

    let report = intake
        .intake(request(
            vec![
                driven_by_a_check(&id, view("view-1", 1)),
                driven_by_a_check(&id, error("boom", json!({}))),
            ],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    assert_eq!(report.accepted, 2);
    assert_eq!(report.sampled, 0);

    let views = batch(&writer, "rum_views").expect("the view is kept");
    let row = &views.events[0].fields;

    assert_eq!(row["session.type"], "synthetics");
    assert_eq!(row["synthetics.test_id"], "check-1");
    assert_eq!(row["synthetics.result_id"], "run-1");

    let errors = batch(&writer, "rum_errors").expect("the error is kept");

    assert!(errors.events[0].fields.get("error.issue_id").is_none());
}

#[tokio::test]
async fn a_filter_that_names_the_kind_of_session_decides_what_becomes_of_a_test() {
    let settings = retention(
        100,
        vec![filter("No checks", 0, "session.type", &["synthetics"])],
        true,
    );

    let (intake, writer) = intake_with(Arc::default(), settings);
    let id = sessions(1).remove(0);

    let report = intake
        .intake(request(
            vec![driven_by_a_check(&id, view("view-1", 1))],
            Some("shop-web"),
            &intake,
        ))
        .await
        .unwrap();

    assert_eq!(report.accepted, 0);
    assert_eq!(report.sampled, 1);
    assert!(batch(&writer, "rum_views").is_none());
}
