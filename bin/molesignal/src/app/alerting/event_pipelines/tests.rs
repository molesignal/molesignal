// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::BTreeMap, sync::Mutex};

use async_trait::async_trait;

use super::{
    notify::{BeforeNotify, NotifyFacts},
    *,
};
use crate::{
    domain::{
        pipeline::alert::AlertConfig,
        query::{QueryRequest, QueryResult},
    },
    infra::runtime::VrlFunctionExecutor,
    shared::{Error, Result},
};

/// The organization's pipelines, as given, and what was recorded of them.
#[derive(Default)]
struct Pipelines {
    pipelines: Vec<AlertPipeline>,
    recorded: Mutex<Vec<(String, AlertOutcome)>>,
}

#[async_trait]
impl AlertPipelineRepository for Pipelines {
    async fn enabled(&self, _org: &Id) -> Result<Vec<AlertPipeline>> {
        Ok(self.pipelines.clone())
    }

    async fn record(&self, pipeline: &Id, outcome: AlertOutcome) -> Result<()> {
        self.recorded
            .lock()
            .unwrap()
            .push((pipeline.to_string(), outcome));
        Ok(())
    }
}

/// VRL as it runs; a model that summarizes an event by its title.
struct Executor(VrlFunctionExecutor);

#[async_trait]
impl FunctionExecutor for Executor {
    async fn run(&self, function: &Function, event: &mut Value) -> Result<FunctionOutcome> {
        self.run_with_metadata(function, event, &Map::new()).await
    }

    async fn run_with_metadata(
        &self,
        function: &Function,
        event: &mut Value,
        metadata: &Map<String, Value>,
    ) -> Result<FunctionOutcome> {
        if function.language != FunctionLanguage::Llm {
            return self.0.run_with_metadata(function, event, metadata).await;
        }

        let answer = format!(
            "{}: {}",
            function.source,
            event["summary"].as_str().unwrap_or("")
        );
        event[SUMMARY_FIELD] = Value::String(answer);
        Ok(FunctionOutcome::Kept)
    }
}

fn pipeline(name: &str, config: Value) -> AlertPipeline {
    let mut config = config;
    config["mode"] = json!("alert");

    AlertPipeline {
        id: Id(format!("p-{name}")),
        org_id: Id("org-a".into()),
        name: name.into(),
        config: AlertConfig::parse(&config).unwrap(),
        updated_at: TimestampMicros(1),
    }
}

fn service(pipelines: Vec<AlertPipeline>) -> (AlertPipelines, Arc<Pipelines>) {
    let repository = Arc::new(Pipelines {
        pipelines,
        ..Pipelines::default()
    });

    (
        AlertPipelines::new(
            repository.clone(),
            Arc::new(Executor(VrlFunctionExecutor::new())),
        ),
        repository,
    )
}

fn incident() -> Incident {
    serde_json::from_value(json!({
        "id": "inc-1",
        "orgId": "org-a",
        "ruleId": "rule-1",
        "escalationPolicyId": "policy-1",
        "status": "open",
        "severity": "error",
        "summary": "Checkout errors",
        "fingerprint": "fp",
        "currentStep": 0,
        "currentLoop": 0,
        "currentStepStartedAt": 0,
        "assignees": [],
        "labels": { "env": "dev", "host_name": "web-01" },
        "annotations": {},
        "traceIds": [],
        "hostIds": [],
        "affectedServices": ["checkout"],
        "triggeringQuery": null,
        "createdAt": 0,
        "acknowledgedAt": null,
        "acknowledgedBy": null,
        "resolvedAt": null,
        "resolvedBy": null,
    }))
    .unwrap()
}

/// Answers every call as told and keeps what was posted.
struct Enricher {
    answer: std::result::Result<Option<Value>, String>,
    posted: Mutex<Vec<(String, Value)>>,
}

#[async_trait]
impl AlertEnricher for Enricher {
    async fn call(
        &self,
        _org: &Id,
        connector: &Id,
        body: &Value,
    ) -> Result<Option<Map<String, Value>>> {
        self.posted
            .lock()
            .unwrap()
            .push((connector.to_string(), body.clone()));

        match &self.answer {
            Ok(answer) => Ok(answer.as_ref().and_then(Value::as_object).cloned()),
            Err(reason) => Err(Error::unavailable(reason.clone())),
        }
    }
}

/// Answers every query with the same rows and keeps what was asked.
struct Queries {
    rows: std::result::Result<Vec<Vec<Value>>, String>,
    asked: Mutex<Vec<QueryRequest>>,
}

#[async_trait]
impl QueryEngine for Queries {
    async fn execute(&self, request: QueryRequest) -> Result<QueryResult> {
        self.asked.lock().unwrap().push(request);

        match &self.rows {
            Ok(rows) => Ok(QueryResult {
                columns: vec!["host".into(), "errors".into()],
                rows: rows.clone(),
                scanned_rows: 0,
                took_ms: 0,
                federation: None,
            }),
            Err(reason) => Err(Error::unavailable(reason.clone())),
        }
    }
}

struct Enriched {
    pipelines: AlertPipelines,
    repository: Arc<Pipelines>,
    enricher: Arc<Enricher>,
    queries: Arc<Queries>,
}

fn enriched(
    pipelines: Vec<AlertPipeline>,
    answer: std::result::Result<Option<Value>, String>,
    rows: std::result::Result<Vec<Vec<Value>>, String>,
) -> Enriched {
    let repository = Arc::new(Pipelines {
        pipelines,
        ..Pipelines::default()
    });
    let enricher = Arc::new(Enricher {
        answer,
        posted: Mutex::new(Vec::new()),
    });
    let queries = Arc::new(Queries {
        rows,
        asked: Mutex::new(Vec::new()),
    });

    Enriched {
        pipelines: AlertPipelines::new(
            repository.clone(),
            Arc::new(Executor(VrlFunctionExecutor::new())),
        )
        .with_enrichment(enricher.clone(), queries.clone()),
        repository,
        enricher,
        queries,
    }
}

const RULE: AlertOrigin<'static> = AlertOrigin {
    source: AlertEventSource::Rule,
    rule_name: "Checkout errors",
};

fn kept(processed: Processed) -> Incident {
    match processed {
        Processed::Kept(incident, _) => *incident,
        Processed::Dropped(reason) => panic!("dropped: {reason}"),
    }
}

fn kept_with_trail(processed: Processed) -> (Incident, Vec<PipelineTrace>) {
    match processed {
        Processed::Kept(incident, trail) => (*incident, trail),
        Processed::Dropped(reason) => panic!("dropped: {reason}"),
    }
}

/// The outcomes recorded so far; recording does not hold the incident back, so wait a little.
async fn recorded(repository: &Pipelines, count: usize) -> Vec<(String, AlertOutcomeKind)> {
    for _ in 0..100 {
        if repository.recorded.lock().unwrap().len() >= count {
            break;
        }

        tokio::task::yield_now().await;
    }

    repository
        .recorded
        .lock()
        .unwrap()
        .iter()
        .map(|(id, outcome)| (id.clone(), outcome.kind))
        .collect()
}

#[tokio::test]
async fn steps_change_the_event_its_pipelines_are_for() {
    let (pipelines, repository) = service(vec![
        pipeline(
            "relabel",
            json!({ "steps": [{ "script": ".labels.host = del(.labels.host_name)\n.labels.team = \"payments\"" }] }),
        ),
        pipeline(
            "external only",
            json!({ "alert": { "sources": ["external"] }, "steps": [{ "script": ".summary = \"never\"" }] }),
        ),
        pipeline(
            "dev is info",
            json!({
                "alert": { "labels": { "env": "dev" } },
                "steps": [{ "script": "if %source == \"rule\" { .severity = \"info\" }\n.annotations.rule = %rule_name" }],
            }),
        ),
    ]);

    let incident = kept(pipelines.process(incident(), RULE).await);

    assert_eq!(
        incident.labels,
        BTreeMap::from([
            ("env".to_string(), "dev".to_string()),
            ("host".to_string(), "web-01".to_string()),
            ("team".to_string(), "payments".to_string()),
        ])
    );
    assert_eq!(incident.summary, "Checkout errors");
    assert_eq!(incident.severity, Severity::Info);
    assert_eq!(
        incident.annotations.get("rule").map(String::as_str),
        Some("Checkout errors")
    );
    // The fingerprint stays: what is one alert does not change with its labels.
    assert_eq!(incident.fingerprint, "fp");
    assert_eq!(
        recorded(&repository, 2).await,
        vec![
            ("p-relabel".to_string(), AlertOutcomeKind::Processed),
            ("p-dev is info".to_string(), AlertOutcomeKind::Processed),
        ]
    );
}

#[tokio::test]
async fn an_abort_drops_the_event_and_says_where() {
    let (pipelines, repository) = service(vec![pipeline(
        "quiet dev",
        json!({ "steps": [{ "transformName": "drop dev", "script": "if .labels.env == \"dev\" { abort }" }] }),
    )]);

    match pipelines.process(incident(), RULE).await {
        Processed::Dropped(reason) => {
            assert_eq!(
                reason,
                "dropped by alert pipeline `quiet dev`, step `drop dev`"
            );
        }
        Processed::Kept(..) => panic!("the event was kept"),
    }

    assert_eq!(
        recorded(&repository, 1).await,
        vec![("p-quiet dev".to_string(), AlertOutcomeKind::Dropped)]
    );
}

#[tokio::test]
async fn a_pipeline_that_fails_leaves_the_event_as_it_came_for_the_next() {
    let (pipelines, repository) = service(vec![
        pipeline(
            "broken",
            json!({ "steps": [
                { "script": ".summary = \"changed\"" },
                { "script": ".count = to_int!(.labels.env)" },
            ] }),
        ),
        pipeline(
            "bad severity",
            json!({ "steps": [{ "script": ".severity = \"urgent\"" }] }),
        ),
        pipeline(
            "bad labels",
            json!({ "steps": [{ "script": ".labels.list = [1, 2]" }] }),
        ),
        pipeline(
            "works",
            json!({ "steps": [{ "script": ".labels.seen = true" }] }),
        ),
    ]);

    let incident = kept(pipelines.process(incident(), RULE).await);

    assert_eq!(incident.summary, "Checkout errors");
    assert_eq!(incident.severity, Severity::Error);
    assert_eq!(
        incident.labels.get("seen").map(String::as_str),
        Some("true")
    );
    assert!(!incident.labels.contains_key("list"));

    let kinds: Vec<AlertOutcomeKind> = recorded(&repository, 4)
        .await
        .into_iter()
        .map(|(_, kind)| kind)
        .collect();

    assert_eq!(
        kinds,
        vec![
            AlertOutcomeKind::Failed,
            AlertOutcomeKind::Failed,
            AlertOutcomeKind::Failed,
            AlertOutcomeKind::Processed,
        ]
    );
}

#[tokio::test]
async fn an_ai_summary_goes_into_its_annotation_when_the_incident_opens_only() {
    let (pipelines, _) = service(vec![pipeline(
        "explain",
        json!({ "steps": [
            { "kind": "ai_summary", "prompt": "Explain", "annotation": "why" },
            { "script": "if .severity == \"critical\" { .severity = \"error\" }" },
        ] }),
    )]);

    let opened = kept(pipelines.process(incident(), RULE).await);

    assert_eq!(
        opened.annotations.get("why").map(String::as_str),
        Some("Explain: Checkout errors")
    );

    // The rule now finds it critical: the steps run again without asking the model, and only the
    // severity they reach counts.
    let mut incoming = incident();
    incoming.severity = Severity::Critical;

    assert_eq!(pipelines.severity(&incoming, RULE).await, Severity::Error);
}

#[tokio::test]
async fn a_new_severity_goes_the_way_the_event_went_when_it_opened() {
    let (pipelines, _) = service(vec![
        pipeline(
            "rename",
            json!({ "steps": [{ "script": ".labels.stage = del(.labels.env)" }] }),
        ),
        pipeline(
            "dev is info",
            json!({ "alert": { "labels": { "stage": "dev" } }, "steps": [{ "script": ".severity = \"info\"" }] }),
        ),
    ]);

    let opened = kept(pipelines.process(incident(), RULE).await);

    assert_eq!(opened.severity, Severity::Info);

    // The rule finds the same alert critical a minute later. It comes in with `env` again, as it
    // did the first time, and the steps take it where they took it then.
    let mut incoming = incident();
    incoming.severity = Severity::Critical;

    assert_eq!(pipelines.severity(&incoming, RULE).await, Severity::Info);
}

#[test]
fn an_event_is_read_back_as_text_maps() {
    let base = incident();

    let changed = changed(
        &base,
        &json!({
            "summary": "  Payments down  ",
            "labels": { "n": 3, "flag": false, "gone": null, " ": "x" },
            "affected_services": ["a", "a", "b"],
        }),
    )
    .unwrap();

    assert_eq!(changed.summary, "Payments down");
    assert_eq!(
        changed.labels,
        BTreeMap::from([
            ("flag".to_string(), "false".to_string()),
            ("n".to_string(), "3".to_string()),
        ])
    );
    assert_eq!(changed.affected_services, vec!["a", "b"]);
    assert_eq!(changed.severity, base.severity);

    for event in [
        json!({ "summary": "" }),
        json!({ "summary": 1 }),
        json!({ "labels": "env=prod" }),
        json!({ "annotations": { "a": { "b": 1 } } }),
        json!({ "affected_services": "checkout" }),
        json!({ "priority": "P10" }),
        json!({ "priority": 1 }),
        json!({ "urgency": "Very urgent!" }),
    ] {
        assert!(super::changed(&base, &event).is_err(), "{event}");
    }
}

#[test]
fn steps_set_and_clear_the_priority_and_the_urgency() {
    use crate::domain::alerting::{priority::Priority, urgency::Urgency};

    let base = incident();
    let set = changed(&base, &json!({ "priority": "p2", "urgency": "low" })).unwrap();

    assert_eq!(set.handling.priority, Some(Priority::P2));
    assert_eq!(set.handling.urgency, Some(Urgency::low()));

    let event = super::event_of(&set);
    assert_eq!(event["priority"], "P2");
    assert_eq!(event["urgency"], "low");

    let cleared = changed(&set, &json!({ "priority": null, "urgency": null })).unwrap();

    assert_eq!(cleared.handling.priority, None);
    assert_eq!(cleared.handling.urgency, None);
}

#[tokio::test]
async fn what_each_pipeline_changed_goes_along_for_the_timeline() {
    let (pipelines, _) = service(vec![
        pipeline(
            "payments",
            json!({ "steps": [{ "script": ".severity = \"critical\"\n.labels.team = \"payments\"\ndel(.labels.host_name)" }] }),
        ),
        pipeline(
            "nothing",
            json!({ "steps": [{ "script": ".labels.env = \"dev\"" }] }),
        ),
        pipeline(
            "broken",
            json!({ "steps": [{ "transformName": "bad", "script": ".severity = \"urgent\"" }] }),
        ),
    ]);

    let (incident, trail) = kept_with_trail(pipelines.process(incident(), RULE).await);

    assert_eq!(incident.severity, Severity::Critical);

    // The one that changed nothing leaves nothing; the one that failed says why.
    let details: Vec<Value> = trail.iter().map(PipelineTrace::detail).collect();

    assert_eq!(
        details,
        vec![
            json!({
                "pipelineId": "p-payments",
                "pipeline": "payments",
                "changes": [
                    { "field": "severity", "from": "error", "to": "critical" },
                    { "field": "labels.host_name", "from": "web-01" },
                    { "field": "labels.team", "to": "payments" },
                ],
                "more": 0,
                "error": null,
            }),
            json!({
                "pipelineId": "p-broken",
                "pipeline": "broken",
                "changes": [],
                "more": 0,
                "error": "`severity` must be info, warning, error or critical",
            }),
        ]
    );
}

#[tokio::test]
async fn a_long_list_of_changes_is_cut_and_counted() {
    let script = (0..MAX_RECORDED_CHANGES + 3)
        .map(|index| format!(".labels.l{index:03} = \"x\""))
        .collect::<Vec<_>>()
        .join("\n");

    let (pipelines, _) = service(vec![pipeline(
        "many",
        json!({ "steps": [{ "script": script }] }),
    )]);

    let (_, trail) = kept_with_trail(pipelines.process(incident(), RULE).await);
    let detail = trail[0].detail();

    assert_eq!(
        detail["changes"].as_array().unwrap().len(),
        MAX_RECORDED_CHANGES
    );
    assert_eq!(detail["more"], 3);
}

#[tokio::test]
async fn a_connector_call_takes_what_the_answer_changes_and_the_steps_go_on_from_there() {
    let call = enriched(
        vec![pipeline(
            "cmdb",
            json!({ "steps": [
                { "transformName": "ask", "kind": "http_enrich", "connectorId": "c-1" },
                { "script": ".annotations.seen = .labels.team" },
            ] }),
        )],
        Ok(Some(json!({
            "severity": "critical",
            "labels": { "team": "payments", "host_name": null },
            "status": "resolved",
            "fingerprint": "other",
        }))),
        Ok(Vec::new()),
    );

    let (opened, trail) = kept_with_trail(call.pipelines.process(incident(), RULE).await);

    assert_eq!(opened.severity, Severity::Critical);
    assert_eq!(
        opened.labels.get("team").map(String::as_str),
        Some("payments")
    );
    assert!(!opened.labels.contains_key("host_name"));
    // What is not the steps' to change stays, whatever the answer says.
    assert_eq!(opened.fingerprint, "fp");
    assert_eq!(opened.status.as_str(), "open");
    assert_eq!(
        opened.annotations.get("seen").map(String::as_str),
        Some("payments")
    );
    assert_eq!(trail[0].detail()["error"], Value::Null);

    let posted = call.enricher.posted.lock().unwrap();

    assert_eq!(posted[0].0, "c-1");
    assert_eq!(posted[0].1["summary"], "Checkout errors");
    assert_eq!(posted[0].1["metadata"]["rule_name"], "Checkout errors");
    assert_eq!(posted[0].1["metadata"]["source"], "rule");
}

#[tokio::test]
async fn a_call_that_fails_leaves_the_event_as_it_was_and_the_other_steps_stand() {
    for answer in [
        Err("no route to host".to_string()),
        // An answer the event could not take is no better.
        Ok(Some(json!({ "severity": "urgent" }))),
    ] {
        let call = enriched(
            vec![pipeline(
                "cmdb",
                json!({ "steps": [
                    { "script": ".labels.before = \"yes\"" },
                    { "transformName": "ask", "kind": "http_enrich", "connectorId": "c-1" },
                    { "script": ".labels.after = \"yes\"" },
                ] }),
            )],
            answer,
            Ok(Vec::new()),
        );

        let (opened, trail) = kept_with_trail(call.pipelines.process(incident(), RULE).await);

        assert_eq!(opened.severity, Severity::Error);
        assert!(opened.labels.contains_key("before") && opened.labels.contains_key("after"));

        let detail = trail[0].detail();

        assert!(
            detail["error"]
                .as_str()
                .unwrap()
                .starts_with("step `ask`: "),
            "{detail}"
        );
        assert_eq!(detail["changes"].as_array().unwrap().len(), 2);

        assert_eq!(
            recorded(&call.repository, 1).await,
            vec![("p-cmdb".to_string(), AlertOutcomeKind::Failed)]
        );
    }
}

#[tokio::test]
async fn an_answer_with_nothing_in_it_changes_nothing() {
    let call = enriched(
        vec![pipeline(
            "cmdb",
            json!({ "steps": [{ "kind": "http_enrich", "connectorId": "c-1" }] }),
        )],
        Ok(None),
        Ok(Vec::new()),
    );

    let (opened, trail) = kept_with_trail(call.pipelines.process(incident(), RULE).await);

    assert_eq!(opened.summary, "Checkout errors");
    assert!(trail.is_empty());
    assert_eq!(
        recorded(&call.repository, 1).await,
        vec![("p-cmdb".to_string(), AlertOutcomeKind::Processed)]
    );
}

fn query_step() -> Value {
    json!({
        "transformName": "errors",
        "kind": "query_enrich",
        "stream": "app_logs",
        "statement": "SELECT host, count(*) AS errors FROM app_logs WHERE host = '{{labels.host_name}}' GROUP BY host",
        "windowSecs": 60,
        "maxRows": 2,
        "annotation": "recent_errors",
    })
}

#[tokio::test]
async fn a_query_step_writes_its_rows_into_an_annotation() {
    let call = enriched(
        vec![pipeline("errors", json!({ "steps": [query_step()] }))],
        Ok(None),
        Ok(vec![
            vec![json!("web-01"), json!(12)],
            vec![json!("web-02"), json!(3)],
            vec![json!("web-03"), json!(1)],
        ]),
    );

    let mut event = incident();
    event.created_at = TimestampMicros(100_000_000);

    let (opened, _) = kept_with_trail(call.pipelines.process(event, RULE).await);

    assert_eq!(
        opened.annotations.get("recent_errors").map(String::as_str),
        Some("host=web-01, errors=12\nhost=web-02, errors=3")
    );

    let asked = call.queries.asked.lock().unwrap();

    assert_eq!(
        asked[0].statement,
        "SELECT host, count(*) AS errors FROM app_logs WHERE host = 'web-01' GROUP BY host"
    );
    assert_eq!(asked[0].org_id, Id("org-a".into()));
    assert_eq!(asked[0].limit, Some(2));
    assert_eq!(asked[0].time_range.start.0, 40_000_000);
    assert_eq!(asked[0].time_range.end.0, 100_000_000);
    assert_eq!(asked[0].stream.as_ref().unwrap().name, "app_logs");
}

#[tokio::test]
async fn a_value_of_the_event_stays_in_the_string_of_the_statement() {
    let call = enriched(
        vec![pipeline("errors", json!({ "steps": [query_step()] }))],
        Ok(None),
        Ok(Vec::new()),
    );

    let mut event = incident();
    event
        .labels
        .insert("host_name".into(), "x' OR '1'='1".into());

    call.pipelines.process(event, RULE).await;

    assert_eq!(
        call.queries.asked.lock().unwrap()[0].statement,
        "SELECT host, count(*) AS errors FROM app_logs WHERE host = 'x'' OR ''1''=''1' GROUP BY host"
    );
}

#[tokio::test]
async fn a_query_with_no_rows_or_no_value_or_no_answer_leaves_the_annotation_out() {
    // No rows: nothing to say, and nothing wrong.
    let empty = enriched(
        vec![pipeline("errors", json!({ "steps": [query_step()] }))],
        Ok(None),
        Ok(Vec::new()),
    );
    let (opened, trail) = kept_with_trail(empty.pipelines.process(incident(), RULE).await);

    assert!(!opened.annotations.contains_key("recent_errors"));
    assert!(trail.is_empty());

    // The event has no such label: the question cannot be asked.
    let mut bare = incident();
    bare.labels.clear();

    let (opened, trail) = kept_with_trail(empty.pipelines.process(bare, RULE).await);

    assert!(!opened.annotations.contains_key("recent_errors"));
    assert!(
        trail[0].detail()["error"]
            .as_str()
            .unwrap()
            .contains("labels.host_name"),
    );
    assert_eq!(empty.queries.asked.lock().unwrap().len(), 1);

    // The engine fails: said, and the event goes on.
    let failing = enriched(
        vec![pipeline("errors", json!({ "steps": [query_step()] }))],
        Ok(None),
        Err("storage unavailable".into()),
    );
    let (opened, trail) = kept_with_trail(failing.pipelines.process(incident(), RULE).await);

    assert_eq!(opened.summary, "Checkout errors");
    assert!(
        trail[0].detail()["error"]
            .as_str()
            .unwrap()
            .contains("storage unavailable")
    );
}

#[tokio::test]
async fn steps_that_go_outside_run_only_when_the_incident_opens() {
    let call = enriched(
        vec![pipeline(
            "mixed",
            json!({ "steps": [
                { "kind": "http_enrich", "connectorId": "c-1" },
                query_step(),
                { "script": "if .severity == \"critical\" { .severity = \"error\" }" },
            ] }),
        )],
        Ok(None),
        Ok(Vec::new()),
    );

    let mut incoming = incident();
    incoming.severity = Severity::Critical;

    assert_eq!(
        call.pipelines.severity(&incoming, RULE).await,
        Severity::Error
    );
    assert!(call.enricher.posted.lock().unwrap().is_empty());
    assert!(call.queries.asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn without_a_way_out_such_a_step_only_says_so() {
    let (pipelines, _) = service(vec![pipeline(
        "mixed",
        json!({ "steps": [
            { "transformName": "ask", "kind": "http_enrich", "connectorId": "c-1" },
            query_step(),
        ] }),
    )]);

    let (_, trail) = kept_with_trail(pipelines.process(incident(), RULE).await);
    let error = trail[0].detail()["error"].as_str().unwrap().to_owned();

    assert!(
        error.contains("step `ask`") && error.contains("step `errors`"),
        "{error}"
    );
}

fn notify_pipeline(name: &str, alert: Value, script: &str) -> AlertPipeline {
    let mut alert = alert;
    alert["stage"] = json!("notify");

    pipeline(
        name,
        json!({ "alert": alert, "steps": [{ "transformName": "the-step", "script": script }] }),
    )
}

fn notify_facts(policy: &Id) -> NotifyFacts<'_> {
    NotifyFacts {
        policy_id: policy,
        policy_name: "On-call",
        rule_id: "rule-1",
        rule_name: "Checkout errors",
        event_name: "triggered",
        at: TimestampMicros(0),
    }
}

#[tokio::test]
async fn each_stage_runs_only_its_own_pipelines() {
    let (pipelines, _) = service(vec![
        pipeline(
            "opening",
            json!({ "steps": [{ "script": ".labels.opening = true" }] }),
        ),
        notify_pipeline("sending", json!({}), ".labels.sending = true"),
    ]);

    let opened = kept(pipelines.process(incident(), RULE).await);

    assert!(opened.labels.contains_key("opening"));
    assert!(!opened.labels.contains_key("sending"));

    let policy = Id("policy-1".into());
    let org = Id("org-a".into());
    let found = pipelines.for_policy(&org, &policy).await;

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "sending");

    let told = pipelines
        .before_notify(&found, AlertFields::of(&incident()), notify_facts(&policy))
        .await;

    let BeforeNotify::Kept(fields) = told else {
        panic!("dropped");
    };

    assert!(fields.labels.contains_key("sending"));
    assert!(!fields.labels.contains_key("opening"));
}

#[tokio::test]
async fn a_pipeline_before_notifications_goes_by_its_policies_and_conditions() {
    let policy = Id("policy-1".into());
    let other = Id("policy-2".into());
    let org = Id("org-a".into());

    let (pipelines, repository) = service(vec![
        notify_pipeline(
            "for one",
            json!({ "policies": ["policy-1"] }),
            ".labels.one = true",
        ),
        notify_pipeline(
            "manual only",
            json!({ "sources": ["manual"] }),
            ".labels.manual = true",
        ),
        notify_pipeline(
            "reads where it is",
            json!({}),
            ".annotations.policy = %policy_name\n.annotations.event = %event\n.annotations.source = %source\n.annotations.rule = %rule_name",
        ),
    ]);

    assert_eq!(pipelines.for_policy(&org, &other).await.len(), 2);

    let found = pipelines.for_policy(&org, &policy).await;

    assert_eq!(found.len(), 3);

    let BeforeNotify::Kept(fields) = pipelines
        .before_notify(&found, AlertFields::of(&incident()), notify_facts(&policy))
        .await
    else {
        panic!("dropped");
    };

    assert!(fields.labels.contains_key("one"));
    assert!(!fields.labels.contains_key("manual"));
    assert_eq!(fields.annotations["policy"], "On-call");
    assert_eq!(fields.annotations["event"], "triggered");
    assert_eq!(fields.annotations["source"], "rule");
    assert_eq!(fields.annotations["rule"], "Checkout errors");

    // An incident opened by hand reads as such.
    let mut manual = incident();
    manual.labels.insert("alert.source".into(), "manual".into());

    let BeforeNotify::Kept(fields) = pipelines
        .before_notify(&found, AlertFields::of(&manual), notify_facts(&policy))
        .await
    else {
        panic!("dropped");
    };

    assert!(fields.labels.contains_key("manual"));
    assert_eq!(fields.annotations["source"], "manual");

    assert_eq!(recorded(&repository, 5).await.len(), 5);
}

#[test]
fn a_drop_before_a_notification_is_told_in_the_workspace_language() {
    let told = |language: &str| {
        notify::drop_reason_text(
            &crate::shared::i18n_language_seed::seeded_pack("alerting.pipeline.", language),
            "quiet",
            "night",
        )
    };

    assert_eq!(
        told("en-us"),
        "dropped by alert pipeline \"quiet\", step \"night\""
    );
    assert_eq!(
        told("zh-cn"),
        "告警流水线「quiet」的步骤「night」丢弃了这条通知"
    );
}

#[tokio::test]
async fn a_drop_before_a_notification_says_where_and_a_failure_changes_nothing() {
    let policy = Id("policy-1".into());
    let org = Id("org-a".into());

    let (pipelines, _) = service(vec![notify_pipeline("quiet", json!({}), "abort")]);
    let found = pipelines.for_policy(&org, &policy).await;

    assert_eq!(
        pipelines
            .before_notify(&found, AlertFields::of(&incident()), notify_facts(&policy))
            .await,
        BeforeNotify::Dropped("quiet / the-step".into())
    );

    let (pipelines, repository) = service(vec![notify_pipeline(
        "broken",
        json!({}),
        ".severity = \"urgent\"",
    )]);
    let found = pipelines.for_policy(&org, &policy).await;
    let before = AlertFields::of(&incident());

    assert_eq!(
        pipelines
            .before_notify(&found, before.clone(), notify_facts(&policy))
            .await,
        BeforeNotify::Kept(Box::new(before))
    );
    assert_eq!(
        recorded(&repository, 1).await,
        vec![("p-broken".to_string(), AlertOutcomeKind::Failed)]
    );
}
