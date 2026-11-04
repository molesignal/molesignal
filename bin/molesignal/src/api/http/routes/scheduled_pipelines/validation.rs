// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::{Value, json};

use super::UpdateReq;
use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::{
        function::FunctionOutcome,
        pipeline::{
            alert::{AlertConfig, AlertStep, EnrichQuery, is_alert, query::known_placeholder},
            processing::{ProcessingKind, ProcessingStep, parse_processing_steps},
            protected::{Guard, RUM_CUSTOM_FIELDS, changed_protected, protected_of},
            realtime::validate_config,
            scheduled::ScheduledConfig,
        },
        query::QueryLanguage,
        rum::event::{RumField, fields as rum_fields},
        stream::{AUDIT_STREAM, FieldType},
    },
    infra::{
        connectors::{ConnectorKind, ConnectorRepository},
        pipeline::{ScheduledPipeline, scheduled::parse_every_secs},
        query::{
            grants::{AUDIT_READ, statement_names_audit},
            parser::template_sample,
        },
        runtime::{VrlRuntime, vrl::runtime::CompiledProgram},
    },
    shared::{Error, Result, ids::Id},
};

/// What a pipeline's configuration names outside itself belongs to the organization.
pub(super) async fn check_references(state: &AppState, config: &Value, org_id: &Id) -> Result<()> {
    check_calendar(config, org_id)?;
    check_enrich_connectors(state.storage.connectors.as_ref(), config, org_id).await
}

/// The holiday calendar an alert pipeline's time condition counts by is the organization's.
fn check_calendar(config: &Value, org_id: &Id) -> Result<()> {
    if !is_alert(config) {
        return Ok(());
    }

    let calendar = AlertConfig::parse(config)?
        .time
        .and_then(|time| time.windows.calendar);

    match calendar {
        Some(calendar) => calendar.check(org_id),
        None => Ok(()),
    }
}

/// Every connector an alert pipeline's HTTP enrich steps call is an HTTP connector of the organization.
async fn check_enrich_connectors(
    connectors: &dyn ConnectorRepository,
    config: &Value,
    org_id: &Id,
) -> Result<()> {
    if !is_alert(config) {
        return Ok(());
    }

    for step in AlertConfig::parse(config)?.steps {
        let AlertStep::Http { connector_id, .. } = step else {
            continue;
        };

        let connector =
            connectors
                .get(org_id, &connector_id)
                .await
                .map_err(|error| match error {
                    Error::NotFound(_) => {
                        Error::invalid(format!("connector `{connector_id}` does not exist"))
                    }
                    other => other,
                })?;

        if connector.kind != ConnectorKind::Http.as_str() {
            return Err(Error::invalid(format!(
                "connector `{}` is not an HTTP connector",
                connector.name
            )));
        }
    }

    Ok(())
}

/// A query enrich step's statement reads as one query, with values of the event only inside its
/// strings, and stays away from the audit trail: it runs where nobody's `audit.read` applies.
fn check_enrich_query(query: &EnrichQuery) -> Result<()> {
    let sample = template_sample(query.language, &query.statement, known_placeholder)?;

    match query.language {
        QueryLanguage::Sql => {
            if statement_names_audit(&sample) {
                return Err(Error::invalid(
                    "a query step cannot read `_audit`: it runs in the background, where nobody's \
                     audit.read applies",
                ));
            }
        }
        QueryLanguage::Promql => {
            promql_parser::parser::parse(&sample)
                .map_err(|error| Error::invalid(format!("the PromQL cannot be read: {error}")))?;
        }
    }

    Ok(())
}

/// A pipeline as it is saved: every step compiles, and a scheduled pipeline's streams, retries
/// and dead letters are what a run can read ([`ScheduledConfig::parse`]). An alert pipeline reads
/// no stream: its conditions and steps are what [`AlertConfig::parse`] reads.
pub(super) fn validate(
    source: &str,
    target: &str,
    config: &Value,
    cron: &str,
    lookback: i32,
) -> Result<()> {
    let runtime = VrlRuntime::new();

    if is_alert(config) {
        for step in AlertConfig::parse(config)?.steps {
            match step {
                AlertStep::Vrl { script, .. } => {
                    runtime
                        .compile(&script)
                        .map_err(|_| Error::invalid("invalid pipeline VRL script"))?;
                }
                AlertStep::Query { query, .. } => check_enrich_query(&query)?,
                AlertStep::Summary { .. } | AlertStep::Http { .. } => {}
            }
        }

        return Ok(());
    }

    let steps = parse_processing_steps(config)?;
    let mut programs = Vec::new();

    for (index, step) in steps.iter().enumerate() {
        if step.kind == ProcessingKind::Vrl {
            let program = runtime
                .compile(&step.script)
                .map_err(|_| Error::invalid("invalid pipeline VRL script"))?;

            programs.push((index, step, program));
        }
    }

    if let Some(setup) = validate_config(config, source, target)? {
        return keeps_protected(&runtime, Guard::of(setup.signal, source), &programs);
    }

    // A profile's metadata changes as it arrives; a scheduled pipeline has nowhere to take it.
    if config.get("signalType").and_then(Value::as_str) == Some("profiles") {
        return Err(Error::invalid(
            "a scheduled pipeline reads logs, metrics or traces; profiles are changed as they arrive",
        ));
    }

    ScheduledConfig::parse(config, source, target)?;

    if parse_every_secs(cron).is_none() || lookback <= 0 {
        return Err(Error::invalid(
            "scheduled pipelines require every:Ns/m/h and a positive lookback",
        ));
    }

    Ok(())
}

/// Whose `audit.read` a pipeline reads the audit trail (`_audit`) with, as `ctx` saves or backfills
/// it: their own, which they must hold, and none when it does not read the trail. Its runs read
/// with that access for as long as it holds ([`crate::app::audit_read`]), so it has to be a person's:
/// a service account's access cannot be checked again.
pub(super) fn audit_reader(ctx: &IamContext, source: &str, config: &Value) -> Result<Option<Id>> {
    if !reads_audit(source, config) {
        return Ok(None);
    }

    if !ctx.has_permission(AUDIT_READ) {
        return Err(Error::forbidden(
            "a pipeline that reads `_audit` needs the audit.read permission",
        ));
    }

    if ctx.credential_service_account_id.is_some() {
        return Err(Error::forbidden(
            "a pipeline reads `_audit` with the access of the person who saves it: save it while \
             signed in or with a personal token, not with a service account",
        ));
    }

    Ok(Some(ctx.user_id.clone()))
}

/// Whose `audit.read` a pipeline reads the audit trail with after this update. Pausing or resuming
/// changes neither what it reads nor where that goes: it stays with whoever decided those. Saving it
/// as it is hands it to a caller who holds the permission, which is how it goes on when the person
/// it reads as has lost theirs; from anyone else such a save changes nothing.
pub(super) fn audit_reader_after(
    ctx: &IamContext,
    existing: &ScheduledPipeline,
    request: &UpdateReq,
) -> Result<Option<Id>> {
    let takes_over = existing.enabled == request.enabled
        && ctx.has_permission(AUDIT_READ)
        && ctx.credential_service_account_id.is_none();

    if takes_over || configuration_changed(existing, request) {
        audit_reader(ctx, request.source_stream.trim(), &request.function_steps)
    } else {
        Ok(existing.audit_reader_id.clone())
    }
}

/// Whether an update changes anything but whether the pipeline is on.
pub(super) fn configuration_changed(existing: &ScheduledPipeline, request: &UpdateReq) -> bool {
    existing.name != request.name
        || existing.source_stream != request.source_stream.trim()
        || existing.target_stream != request.target_stream.trim()
        || existing.function_steps != request.function_steps
        || existing.cron != request.cron
        || existing.lookback_secs != request.lookback_secs
}

fn reads_audit(source: &str, config: &Value) -> bool {
    source == AUDIT_STREAM
        || config
            .get("sources")
            .and_then(Value::as_array)
            .is_some_and(|sources| {
                sources
                    .iter()
                    .any(|item| item.as_str() == Some(AUDIT_STREAM))
            })
}

/// A realtime pipeline on metrics, traces or RUM leaves what its data rests on as it arrives
/// ([`crate::domain::pipeline::protected`]). Each step runs once on an event that has all of it,
/// and a step that changes a kept field is refused here instead of failing every event later; so
/// is a step that can drop a RUM view or vital (`abort`). A step that changes a kept field only
/// under some condition is caught as events arrive.
fn keeps_protected(
    runtime: &VrlRuntime,
    guard: Option<Guard>,
    programs: &[(usize, &ProcessingStep, CompiledProgram)],
) -> Result<()> {
    let Some(guard) = guard else {
        return Ok(());
    };

    let probe = probe(guard);
    let before = protected_of(guard, probe.as_object().unwrap_or(&Default::default()));

    for (index, step, program) in programs {
        let name = match step.transform_name.trim() {
            "" => (index + 1).to_string(),
            name => name.to_owned(),
        };

        if !guard.drops() && program.can_abort() {
            return Err(Error::invalid(format!(
                "step `{name}` can drop an event (`abort`); a pipeline on {} keeps every row",
                guard.name()
            )));
        }

        let mut event = probe.clone();

        // A step that drops the probe, or fails on it, says nothing about these fields.
        if !matches!(runtime.run(program, &mut event), Ok(FunctionOutcome::Kept)) {
            continue;
        }

        if let Some(field) = changed_protected(guard, &before, &event) {
            return Err(Error::invalid(format!(
                "step `{name}` changes `{field}`; a realtime pipeline on {} keeps it as it arrives",
                guard.name()
            )));
        }
    }

    Ok(())
}

/// An event of the guarded data with every field a pipeline may not change.
fn probe(guard: Guard) -> Value {
    match guard {
        Guard::Metrics => json!({
            "metric_name": "probe_seconds",
            "value": 1.5,
            "metric_kind": "histogram",
            "metric_description": "",
            "metric_unit": "s",
            "metric_temporality": "cumulative",
            "metric_monotonic": false,
            "metric_start_time_unix_nano": 1,
            "count": 4,
            "sum": 210.5,
            "min": 5.0,
            "max": 150.0,
            "le": "0.5",
            "quantile": "0.99",
            "service": "probe",
        }),
        Guard::Profiles => json!({
            "id": "probe",
            "service": "probe",
            "profileType": "cpu",
            "objectKey": "blobs/v1/probe/profiles/probe/cpu/19700101/probe.pprof.zst",
            "archivedBytes": 1,
            "duration_nanos": 1,
            "sampleCount": 1,
            "totalValue": 1,
            "unsymbolized": false,
            "trace_id": "",
            "span_id": "",
            "labels": {},
        }),
        Guard::Spans => json!({
            "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
            "span_id": "00f067aa0ba902b7",
            "parent_span_id": "53995c3f42cd8ad8",
            "service.name": "probe",
            "name": "probe",
            "start_time_unix_nano": 1,
            "end_time_unix_nano": 2,
            "duration_ns": 1,
        }),
        Guard::Rum(stream) => Value::Object(
            rum_fields(stream)
                .iter()
                .map(|field| (field.name.to_string(), rum_sample(field)))
                .collect(),
        ),
    }
}

/// A value of a RUM column's type. The custom parts are empty objects, for steps to add to.
fn rum_sample(field: &RumField) -> Value {
    if RUM_CUSTOM_FIELDS.contains(&field.name) {
        return json!({});
    }

    match field.data_type {
        FieldType::Bool => json!(true),
        FieldType::Int64 | FieldType::Timestamp => json!(1),
        FieldType::Float64 => json!(1.5),
        FieldType::Utf8 => json!("probe"),
        FieldType::Json => json!(["probe"]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn realtime(signal: &str, scripts: &[&str]) -> Value {
        let steps: Vec<Value> = scripts
            .iter()
            .map(|script| json!({ "transformName": "", "script": script }))
            .collect();

        json!({ "mode": "realtime", "signalType": signal, "steps": steps })
    }

    fn save(config: &Value) -> Result<()> {
        validate("prod", "prod", config, "", 0)
    }

    #[test]
    fn a_rum_pipeline_changes_custom_parts_and_never_drops_a_view() {
        let rum = |source: &str, script: &str| {
            let config = json!({
                "mode": "realtime",
                "signalType": "logs",
                "steps": [{ "transformName": "tidy", "script": script }],
            });

            validate(source, source, &config, "", 0)
        };

        assert!(rum("rum_views", ".context.team = \"checkout\"").is_ok());

        let error = rum("rum_views", ".\"view.url\" = \"/cart\"")
            .unwrap_err()
            .to_string();
        assert!(error.contains("changes `view.url`"), "{error}");

        let error = rum("rum_views", "if .\"view.name\" == \"bot\" { abort }")
            .unwrap_err()
            .to_string();
        assert!(error.contains("can drop an event"), "{error}");

        assert!(rum("rum_actions", "if .\"action.type\" == \"scroll\" { abort }").is_ok());
        assert!(rum("rum_sessions", ".context.team = \"checkout\"").is_err());
    }

    fn caller(permissions: &[&str], service_account: bool) -> IamContext {
        IamContext {
            user_id: Id::from_string("user-1"),
            org_id: Id::from_string("org-1"),
            display_role: String::new(),
            roles: Vec::new(),
            credential_token_id: None,
            credential_application_id: None,
            credential_service_account_id: service_account.then(|| Id::from_string("sa-1")),
            scope: Default::default(),
            permissions: permissions.iter().map(|key| (*key).to_string()).collect(),
            policy_version: 0,
        }
    }

    #[test]
    fn a_pipeline_reads_the_audit_trail_with_the_access_of_the_person_who_saves_it() {
        let reader = caller(&[AUDIT_READ], false);
        let config = json!({ "sources": ["app_logs", "_audit"] });
        let only_logs = json!({ "sources": ["app_logs"] });

        assert_eq!(
            audit_reader(&reader, "app_logs", &config).unwrap(),
            Some(Id::from_string("user-1"))
        );
        assert_eq!(
            audit_reader(&reader, "_audit", &json!({})).unwrap(),
            Some(Id::from_string("user-1"))
        );
        assert_eq!(audit_reader(&reader, "app_logs", &only_logs).unwrap(), None);

        let other = caller(&[], false);

        assert!(audit_reader(&other, "app_logs", &config).is_err());
        assert!(audit_reader(&other, "_audit", &json!({})).is_err());
        assert_eq!(audit_reader(&other, "app_logs", &only_logs).unwrap(), None);

        // A service account holds the permission, but nobody can check it again at a run.
        assert!(audit_reader(&caller(&[AUDIT_READ], true), "_audit", &json!({})).is_err());
    }

    fn reading_the_trail_as_ada() -> ScheduledPipeline {
        ScheduledPipeline {
            id: Id::from_string("pipeline-1"),
            org_id: Id::from_string("org-1"),
            name: "copy".into(),
            source_stream: "_audit".into(),
            target_stream: "audit_copy".into(),
            function_steps: json!({ "mode": "scheduled" }),
            cron: "every:5m".into(),
            lookback_secs: 300,
            last_run_at: None,
            run_state: json!({}),
            audit_reader_id: Some(Id::from_string("ada")),
            enabled: true,
            created_at: crate::shared::time::TimestampMicros(0),
            updated_at: crate::shared::time::TimestampMicros(0),
        }
    }

    fn update_of(pipeline: &ScheduledPipeline) -> UpdateReq {
        UpdateReq {
            name: pipeline.name.clone(),
            source_stream: pipeline.source_stream.clone(),
            target_stream: pipeline.target_stream.clone(),
            function_steps: pipeline.function_steps.clone(),
            cron: pipeline.cron.clone(),
            lookback_secs: pipeline.lookback_secs,
            enabled: pipeline.enabled,
        }
    }

    #[test]
    fn an_update_says_whose_access_the_pipeline_reads_the_audit_trail_with() {
        let existing = reading_the_trail_as_ada();
        let holder = caller(&[AUDIT_READ], false);
        let other = caller(&[], false);
        let service_account = caller(&[AUDIT_READ], true);
        let ada = Some(Id::from_string("ada"));
        let user = Some(Id::from_string("user-1"));

        // Saved as it is: a holder takes it over, which is how a stopped pipeline goes on.
        let same = update_of(&existing);
        assert_eq!(audit_reader_after(&holder, &existing, &same).unwrap(), user);
        assert_eq!(audit_reader_after(&other, &existing, &same).unwrap(), ada);
        assert_eq!(
            audit_reader_after(&service_account, &existing, &same).unwrap(),
            ada
        );

        // Paused or resumed: it stays with whoever decided what it reads, whoever does this.
        let paused = UpdateReq {
            enabled: false,
            ..update_of(&existing)
        };
        assert_eq!(
            audit_reader_after(&holder, &existing, &paused).unwrap(),
            ada
        );
        assert_eq!(audit_reader_after(&other, &existing, &paused).unwrap(), ada);

        // Changed: it takes the caller's access, and only a holder's.
        let changed = UpdateReq {
            target_stream: "elsewhere".into(),
            ..update_of(&existing)
        };
        assert_eq!(
            audit_reader_after(&holder, &existing, &changed).unwrap(),
            user
        );
        assert!(audit_reader_after(&other, &existing, &changed).is_err());
        assert!(audit_reader_after(&service_account, &existing, &changed).is_err());
    }

    #[test]
    fn a_step_on_profiles_leaves_where_the_object_is() {
        let profile = |script: &str| {
            validate(
                "default",
                "default",
                &json!({
                    "mode": "realtime",
                    "signalType": "profiles",
                    "steps": [{ "transformName": "tidy", "script": script }],
                }),
                "",
                0,
            )
        };

        assert!(profile(".labels.env = \"prod\"").is_ok());
        assert!(profile("if .service == \"noisy\" { abort }").is_ok());

        let error = profile(".object_key = \"elsewhere\"")
            .unwrap_err()
            .to_string();
        assert!(error.contains("changes `object_key`"), "{error}");

        let scheduled = json!({ "mode": "scheduled", "signalType": "profiles", "steps": [] });
        assert!(validate("default", "copy", &scheduled, "every:5m", 300).is_err());
    }

    #[test]
    fn a_query_step_is_one_query_that_keeps_its_values_in_strings() {
        let save = |language: &str, stream_type: &str, statement: &str| {
            validate(
                "",
                "",
                &json!({
                    "mode": "alert",
                    "steps": [{
                        "kind": "query_enrich",
                        "language": language,
                        "stream": "s",
                        "streamType": stream_type,
                        "statement": statement,
                    }],
                }),
                "",
                0,
            )
        };

        assert!(
            save(
                "sql",
                "logs",
                "SELECT count(*) FROM s WHERE h = '{{labels.host}}'"
            )
            .is_ok()
        );
        assert!(save("promql", "metrics", "up{i=\"{{labels.host}}\"}").is_ok());

        for (language, stream_type, statement, says) in [
            (
                "sql",
                "logs",
                "SELECT 1 FROM s WHERE h = {{labels.host}}",
                "inside a string",
            ),
            (
                "sql",
                "logs",
                "SELECT 1 FROM s WHERE h = '{{fingerprint}}'",
                "not a value",
            ),
            ("sql", "logs", "DELETE FROM s", "single query"),
            ("sql", "logs", "SELECT 1 FROM _audit", "_audit"),
            (
                "sql",
                "logs",
                "SELECT 1 FROM s WHERE x IN (SELECT y FROM _audit)",
                "_audit",
            ),
            (
                "promql",
                "metrics",
                "up{i={{labels.host}}}",
                "inside a string",
            ),
            ("promql", "metrics", "sum(", "cannot be read"),
        ] {
            let error = save(language, stream_type, statement)
                .unwrap_err()
                .to_string();

            assert!(error.contains(says), "{statement}: {error}");
        }
    }

    #[test]
    fn a_step_that_changes_what_a_metric_or_span_rests_on_is_refused() {
        assert!(save(&realtime("metrics", &["del(.pod)", ".env = \"prod\""])).is_ok());
        assert!(save(&realtime("traces", &["del(.\"db.statement\")"])).is_ok());

        for (signal, script, field) in [
            ("metrics", ".metric_name = \"requests\"", "metric_name"),
            ("metrics", "del(.le)", "le"),
            ("metrics", ".value = 0", "value"),
            ("metrics", "del(.sum)", "sum"),
            ("traces", "del(.parent_span_id)", "parent_span_id"),
            ("traces", ".\"service.name\" = \"web\"", "service.name"),
        ] {
            let error = save(&realtime(signal, &["del(.pod)", script]))
                .unwrap_err()
                .to_string();

            assert!(
                error.contains(&format!("step `2` changes `{field}`")),
                "{error}"
            );
        }

        // A change made only under a condition the probe does not meet is left to the events.
        assert!(
            save(&realtime(
                "metrics",
                &["if .env == \"dev\" { .metric_name = \"dev\" }"]
            ))
            .is_ok()
        );
        // Logs keep no field.
        assert!(save(&realtime("logs", &[".trace_id = \"t\""])).is_ok());
    }
}
