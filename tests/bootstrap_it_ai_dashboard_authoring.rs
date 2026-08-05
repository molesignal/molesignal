// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! AI Dashboard authoring control-plane integration coverage.

mod common;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use molesignal::{
    app::dashboard::{
        authoring::DashboardAuthoringCompiler,
        contract_registry::{
            DashboardContractRegistryService, DashboardContractResolver, ResolvedDashboardContracts,
        },
    },
    domain::{
        dashboard::{
            Folder,
            authoring::{
                DashboardAuthoringSpec, DashboardDraft, DashboardDraftRepository,
                DashboardDraftStatus, PreflightReport,
            },
        },
        stream::StreamType,
    },
    infra::persistence::repositories::{
        dashboard_authoring::PgDashboardDraftRepository,
        dashboard_contract_registry::PgDashboardContractRepository,
        intelligence::toolsets::AgentToolset,
    },
    intelligence::{
        model::{AgentProfile, NetworkAccess},
        tool_control::{ToolExecutionMode, ToolPolicy},
    },
    shared::{LicenseGate, ids::Id, time::TimestampMicros},
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

const VALID_AUTHORING: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/dashboard/fixtures/valid/authoring-v1-promql.json"
));

struct IntelligenceLicense;

impl LicenseGate for IntelligenceLicense {
    fn has_feature(&self, name: &str) -> bool {
        name == "intelligence"
    }

    fn add_ingest_bytes(&self, _n: u64) -> bool {
        true
    }

    fn expired(&self, _now_micros: i64) -> bool {
        false
    }

    fn issued_to(&self) -> &str {
        "dashboard-authoring-test"
    }

    fn reset_daily(&self) {}

    fn features(&self) -> Vec<String> {
        vec!["intelligence".into()]
    }
}

fn draft(
    id: &str,
    org_id: Id,
    actor: Id,
    folder_id: Option<Id>,
    expires_at: TimestampMicros,
    contracts: &ResolvedDashboardContracts,
) -> DashboardDraft {
    let spec: DashboardAuthoringSpec = serde_json::from_str(VALID_AUTHORING).unwrap();
    let compiled = DashboardAuthoringCompiler::compile(&spec).unwrap();
    DashboardDraft {
        id: Id(id.into()),
        org_id,
        created_by: actor,
        authoring_version: spec.authoring_version,
        model_schema_version: compiled.dashboard_model_version,
        compiler_version: compiled.compiler_version,
        contract_binding_revision: contracts.binding.revision,
        authoring_schema_hash: contracts.binding.selection.authoring.schema_hash.clone(),
        model_schema_hash: contracts.binding.selection.model.schema_hash.clone(),
        visualization_schema_hash: contracts
            .binding
            .selection
            .visualization
            .schema_hash
            .clone(),
        authoring_spec: serde_json::to_value(spec).unwrap(),
        compiled_model: compiled.model,
        model_hash: compiled.model_hash,
        folder_id,
        status: DashboardDraftStatus::Ready,
        dashboard_id: None,
        warnings: Vec::new(),
        preflight: PreflightReport::default(),
        created_at: TimestampMicros::now(),
        expires_at,
        consumed_at: None,
    }
}

fn policy(server: &common::TestServer, mode: ToolExecutionMode, enabled: bool) -> ToolPolicy {
    let now = TimestampMicros::now();
    ToolPolicy {
        org_id: server.root_org_id.clone(),
        tool_name: "propose_dashboard_creation".into(),
        enabled,
        execution_mode: mode,
        environment_overrides: json!({}),
        timeout_ms: 30_000,
        max_calls_per_run: 4,
        max_response_bytes: 1_048_576,
        updated_by: server.root_user_id.clone(),
        created_at: now,
        updated_at: now,
    }
}

async fn propose(server: &common::TestServer, draft: &DashboardDraft) -> reqwest::Response {
    server
        .client
        .post(format!(
            "{}/api/v1/intelligence/dashboard-drafts/{}/propose",
            server.base_url, draft.id.0
        ))
        .header(server.auth_header().0, server.auth_header().1)
        .json(&json!({
            "expected_hash": draft.model_hash,
            "reason": "Create a reviewed operations Dashboard",
            "impact": "Adds one native Dashboard"
        }))
        .send()
        .await
        .unwrap()
}

async fn execute(
    server: &common::TestServer,
    approval_id: &str,
    idempotency_key: &str,
) -> reqwest::Response {
    server
        .client
        .post(format!(
            "{}/api/v1/intelligence/approvals/{approval_id}/execute",
            server.base_url
        ))
        .header(server.auth_header().0, server.auth_header().1)
        .json(&json!({"idempotency_key": idempotency_key}))
        .send()
        .await
        .unwrap()
}

async fn set_policy(server: &common::TestServer, mode: ToolExecutionMode, enabled: bool) {
    server
        .state
        .intelligence
        .tool_control
        .upsert_policy(policy(server, mode, enabled))
        .await
        .unwrap();
}

fn openai_tool_response(id: &str, name: &str, arguments: Value) -> ResponseTemplate {
    let tool_call = json!({
        "choices": [{
            "index": 0,
            "delta": {
                "tool_calls": [{
                    "index": 0,
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments.to_string(),
                    }
                }]
            },
            "finish_reason": null
        }]
    });
    let done = json!({
        "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]
    });
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(format!(
            "data: {tool_call}\n\ndata: {done}\n\ndata: [DONE]\n\n"
        ))
}

fn openai_text_response(summary: &str) -> ResponseTemplate {
    let answer = json!({
        "summary": summary,
        "evidence": [],
        "likely_causes": [],
        "limitations": [],
        "suggested_next_steps": [],
        "related_links": [],
        "confidence": "high"
    })
    .to_string();
    let chunk = json!({
        "choices": [{
            "index": 0,
            "delta": {"content": answer},
            "finish_reason": null
        }]
    });
    let done = json!({
        "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 12, "completion_tokens": 6}
    });
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(format!("data: {chunk}\n\ndata: {done}\n\ndata: [DONE]\n\n"))
}

fn tool_json_result(sse: &str, call_id: &str) -> Value {
    for block in sse.split("\n\n") {
        let mut event = None;
        let mut data = None;
        for line in block.lines() {
            event = line.strip_prefix("event: ").or(event);
            data = line.strip_prefix("data: ").or(data);
        }
        if event != Some("tool_end") {
            continue;
        }
        let payload: Value = serde_json::from_str(data.expect("tool_end data")).unwrap();
        if payload["id"] != call_id {
            continue;
        }
        assert_eq!(payload["is_error"], false, "tool failed: {payload}");
        let content: Value =
            serde_json::from_str(payload["result"].as_str().expect("serialized tool result"))
                .unwrap();
        return content[0]["json"].clone();
    }
    panic!("missing tool result for {call_id}: {sse}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dashboard_authoring_control_plane_is_tenant_safe_and_exactly_once() {
    if common::skip_unless_enabled() {
        eprintln!("skipping AI Dashboard control integration (set MS_RUN_IT=1)");
        return;
    }
    let server = common::TestServer::start().await;
    server
        .state
        .platform
        .license_holder
        .replace(Arc::new(IntelligenceLicense));
    let pool = PgPool::connect(&server.settings.store.meta.dsn)
        .await
        .unwrap();
    let drafts = PgDashboardDraftRepository::new(pool.clone());
    let contract_registry = DashboardContractRegistryService::new(Arc::new(
        PgDashboardContractRepository::new(pool.clone()),
    ));
    let contracts = contract_registry.active().await.unwrap();
    let now = TimestampMicros::now();
    let valid_until = TimestampMicros(now.0 + 30 * 60 * 1_000_000);

    let profile_draft = draft(
        "profile-disabled-draft",
        server.root_org_id.clone(),
        server.root_user_id.clone(),
        None,
        valid_until,
        &contracts,
    );
    drafts.create(profile_draft.clone()).await.unwrap();
    let profile_now = TimestampMicros::now();
    let mut profile = server
        .state
        .intelligence
        .repository
        .create_profile(AgentProfile {
            id: Id::new(),
            org_id: server.root_org_id.clone(),
            name: "preview-only".into(),
            description: "Dashboard preparation without proposal".into(),
            model_provider_id: None,
            model: None,
            allowed_tools: vec!["prepare_dashboard".into()],
            data_scope: json!({}),
            risk_policy: json!({}),
            network_access: NetworkAccess::Blocked,
            max_context_tokens: 32_000,
            max_investigation_secs: 1_800,
            max_tool_calls: 32,
            is_default: true,
            enabled: true,
            created_by: server.root_user_id.clone(),
            created_at: profile_now,
            updated_at: profile_now,
        })
        .await
        .unwrap();
    assert_eq!(propose(&server, &profile_draft).await.status(), 403);
    profile.enabled = false;
    profile.updated_at = TimestampMicros::now();
    server
        .state
        .intelligence
        .repository
        .update_profile(profile)
        .await
        .unwrap();

    let toolset = server
        .state
        .intelligence
        .toolsets
        .create(AgentToolset {
            id: Id::new(),
            org_id: server.root_org_id.clone(),
            name: "preview-only".into(),
            schema: json!({"builtin": ["prepare_dashboard"]}),
            enabled: true,
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();
    assert_eq!(propose(&server, &profile_draft).await.status(), 403);
    server
        .state
        .intelligence
        .toolsets
        .delete(&server.root_org_id, &toolset.id)
        .await
        .unwrap();

    set_policy(&server, ToolExecutionMode::Disabled, false).await;
    assert_eq!(propose(&server, &profile_draft).await.status(), 403);

    for (mode, required) in [
        (ToolExecutionMode::SingleApproval, 1),
        (ToolExecutionMode::DualApproval, 2),
    ] {
        set_policy(&server, mode, true).await;
        let tightened = draft(
            &format!("tightened-{required}"),
            server.root_org_id.clone(),
            server.root_user_id.clone(),
            None,
            valid_until,
            &contracts,
        );
        drafts.create(tightened.clone()).await.unwrap();
        let response = propose(&server, &tightened).await;
        assert_eq!(response.status(), 200);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["approval"]["status"], "pending");
        assert_eq!(body["approval"]["required_approvals"], required);
    }

    set_policy(&server, ToolExecutionMode::Confirmation, true).await;
    let expired = draft(
        "expired-draft",
        server.root_org_id.clone(),
        server.root_user_id.clone(),
        None,
        TimestampMicros(now.0 - 1),
        &contracts,
    );
    drafts.create(expired.clone()).await.unwrap();
    assert!(!propose(&server, &expired).await.status().is_success());

    let foreign = draft(
        "foreign-draft",
        Id("another-org".into()),
        server.root_user_id.clone(),
        None,
        valid_until,
        &contracts,
    );
    drafts.create(foreign.clone()).await.unwrap();
    assert_eq!(propose(&server, &foreign).await.status(), 404);

    let foreign_folder = Folder {
        id: Id::new(),
        org_id: Id("another-org".into()),
        name: "Foreign".into(),
        parent_id: None,
    };
    server
        .state
        .dashboard
        .folders()
        .create(foreign_folder.clone())
        .await
        .unwrap();
    let invalid_folder = draft(
        "foreign-folder-draft",
        server.root_org_id.clone(),
        server.root_user_id.clone(),
        Some(foreign_folder.id),
        valid_until,
        &contracts,
    );
    drafts.create(invalid_folder.clone()).await.unwrap();
    let response = propose(&server, &invalid_folder).await;
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    let approval_id = body["approval"]["id"].as_str().unwrap();
    let response = execute(&server, approval_id, "foreign-folder-key").await;
    assert_eq!(response.status(), 200);
    let execution: Value = response.json().await.unwrap();
    assert_eq!(execution["status"], "failed");

    let mut instance_settings = server.state.iam.instance_settings.get().await.unwrap();
    instance_settings.federation_cluster_id = "dashboard-authoring-test-cluster".into();
    instance_settings.updated_at = TimestampMicros::now();
    server
        .state
        .iam
        .instance_settings
        .update(instance_settings)
        .await
        .unwrap();

    let ready = draft(
        "ready-concurrent-draft",
        server.root_org_id.clone(),
        server.root_user_id.clone(),
        None,
        valid_until,
        &contracts,
    );
    drafts.create(ready.clone()).await.unwrap();
    let response = propose(&server, &ready).await;
    assert_eq!(response.status(), 200);
    let proposal: Value = response.json().await.unwrap();
    assert_eq!(proposal["approval"]["status"], "approved");
    assert_eq!(proposal["approval"]["required_approvals"], 0);
    assert!(
        proposal["approval"]["expires_at"]
            .as_i64()
            .is_some_and(|expires| expires <= ready.expires_at.0)
    );
    let approval_id = proposal["approval"]["id"].as_str().unwrap().to_string();

    let preview = server
        .client
        .get(format!(
            "{}/api/v1/intelligence/dashboard-drafts/{}",
            server.base_url, ready.id.0
        ))
        .header(server.auth_header().0, server.auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(preview.status(), 200);
    let preview: Value = preview.json().await.unwrap();
    assert_eq!(preview["status"], "ready");
    assert_eq!(preview["operation"]["status"], "approved");

    let left = execute(&server, &approval_id, "concurrent-key-left");
    let right = execute(&server, &approval_id, "concurrent-key-right");
    let (left, right) = tokio::join!(left, right);
    assert_eq!(left.status(), 200);
    assert_eq!(right.status(), 200);
    let left: Value = left.json().await.unwrap();
    let right: Value = right.json().await.unwrap();
    assert_eq!(left["id"], right["id"]);
    let execution_id = left["id"].as_str().unwrap();

    let fetched = server
        .client
        .get(format!(
            "{}/api/v1/intelligence/executions/{execution_id}",
            server.base_url
        ))
        .header(server.auth_header().0, server.auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(fetched.status(), 200);
    let fetched: Value = fetched.json().await.unwrap();
    assert_eq!(fetched["status"], "succeeded");
    assert_eq!(fetched["verification"]["draft_consumed"], true);
    let dashboard_route = fetched["verification"]["dashboard_route"].as_str().unwrap();

    for key in ["concurrent-key-left", "late-different-key"] {
        let replay = execute(&server, &approval_id, key).await;
        assert_eq!(replay.status(), 200);
        let replay: Value = replay.json().await.unwrap();
        assert_eq!(replay["id"], fetched["id"]);
    }

    let dashboard_id = fetched["verification"]["dashboard_id"].as_str().unwrap();
    let response = server
        .client
        .get(format!(
            "{}/api/v1/dashboards/{dashboard_id}",
            server.base_url
        ))
        .header(server.auth_header().0, server.auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "route {dashboard_route}");

    let execution_count = sqlx::query(
        "SELECT COUNT(*) AS count FROM intelligence_executions
         WHERE org_id = $1 AND approval_request_id = $2",
    )
    .bind(&server.root_org_id.0)
    .bind(&approval_id)
    .fetch_one(&pool)
    .await
    .unwrap()
    .try_get::<i64, _>("count")
    .unwrap();
    let dashboard_count = sqlx::query("SELECT COUNT(*) AS count FROM dashboards WHERE org_id = $1")
        .bind(&server.root_org_id.0)
        .fetch_one(&pool)
        .await
        .unwrap()
        .try_get::<i64, _>("count")
        .unwrap();
    let federation_count = sqlx::query_scalar::<i64>(
        "SELECT COUNT(*) FROM cluster_event_outbox
         WHERE org_id = $1 AND event_type = 'com.molesignal.dashboard.created'",
    )
    .bind(&server.root_org_id.0)
    .fetch_one(&pool)
    .await
    .unwrap();
    let audit_rows = sqlx::query(
        "SELECT action, payload FROM audit_events
         WHERE org_id = $1 AND (
           (action = 'dashboard.created_from_ai_draft' AND payload->>'draft_id' = $2)
           OR (action = 'intelligence.execution.completed' AND payload->>'target' = $2)
         )
         ORDER BY action",
    )
    .bind(&server.root_org_id.0)
    .bind(&ready.id.0)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(execution_count, 1);
    assert_eq!(dashboard_count, 1);
    assert_eq!(federation_count, 1);
    assert_eq!(audit_rows.len(), 2);
    for row in audit_rows {
        let payload: sqlx::types::Json<Value> = row.try_get("payload").unwrap();
        let serialized = payload.0.to_string();
        assert!(!serialized.contains("compiled_model"));
        assert!(!serialized.contains("elements"));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dashboard_authoring_runs_from_chat_intent_to_renderable_dashboard() {
    if common::skip_unless_enabled() {
        eprintln!("skipping AI Dashboard chat E2E (set MS_RUN_IT=1)");
        return;
    }
    let server = common::TestServer::start().await;
    server
        .state
        .platform
        .license_holder
        .replace(Arc::new(IntelligenceLicense));
    set_policy(&server, ToolExecutionMode::Confirmation, true).await;

    common::seed_stream(
        &server.state,
        &server.root_org_id,
        "app_errors",
        StreamType::Logs,
    )
    .await;
    let response = server
        .client
        .post(format!("{}/api/v1/ingest/logs/app_errors", server.base_url))
        .header(server.auth_header().0, server.auth_header().1)
        .json(&json!([{
            "_timestamp": TimestampMicros::now().0,
            "msg": "request failed"
        }]))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let pool = PgPool::connect(&server.settings.store.meta.dsn)
        .await
        .unwrap();
    let flush_pool = pool.clone();
    let flush_org = server.root_org_id.0.clone();
    assert!(
        common::wait_until_async(10, move || {
            let pool = flush_pool.clone();
            let org_id = flush_org.clone();
            async move {
                sqlx::query_scalar::<i64>(
                    "SELECT COUNT(*) FROM parquet_file_meta
                     WHERE org_id = $1 AND stream = 'app_errors' AND deleted = FALSE",
                )
                .bind(org_id)
                .fetch_one(&pool)
                .await
                .unwrap_or_default()
                    > 0
            }
        })
        .await,
        "ingested stream did not flush in time"
    );

    let provider_calls = Arc::new(AtomicUsize::new(0));
    let proposal_reference = Arc::new(Mutex::new(None::<(String, String)>));
    let openai = MockServer::start().await;
    let calls_for_mock = provider_calls.clone();
    let reference_for_mock = proposal_reference.clone();
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(move |_request: &wiremock::Request| {
            match calls_for_mock.fetch_add(1, Ordering::SeqCst) {
                0 => openai_tool_response(
                    "e2e_streams",
                    "list_streams",
                    json!({"stream_type": "logs"}),
                ),
                1 => openai_tool_response(
                    "e2e_prepare",
                    "prepare_dashboard",
                    json!({
                        "authoringVersion": 1,
                        "title": "Application error overview",
                        "timeRange": {"from": "now-1h", "to": "now", "timezone": "browser"},
                        "elements": [{
                            "kind": "panel",
                            "title": "Recent errors",
                            "size": "full",
                            "visualization": {"type": "table"},
                            "queries": [{
                                "kind": "sql",
                                "stream": "app_errors",
                                "statement": "SELECT msg FROM app_errors",
                                "format": "table"
                            }]
                        }]
                    }),
                ),
                2 => openai_text_response("The persisted Dashboard preview is ready for review."),
                3 => {
                    let (draft_id, expected_hash) = reference_for_mock
                        .lock()
                        .unwrap()
                        .clone()
                        .expect("test populated the reviewed draft reference");
                    openai_tool_response(
                        "e2e_propose",
                        "propose_dashboard_creation",
                        json!({
                            "draft_id": draft_id,
                            "expected_hash": expected_hash,
                            "reason": "Create the reviewed application error Dashboard",
                            "impact": "Adds one native Dashboard"
                        }),
                    )
                }
                4 => openai_text_response("The reviewed creation request is ready to execute."),
                unexpected => ResponseTemplate::new(500)
                    .set_body_string(format!("unexpected provider call {unexpected}")),
            }
        })
        .mount(&openai)
        .await;

    let provider = server
        .state
        .intelligence
        .model_providers
        .create(
            molesignal::infra::persistence::repositories::intelligence::model_providers::ModelProviderInput {
                id: Id::new(),
                org_id: server.root_org_id.clone(),
                provider: "openai".into(),
                name: "dashboard-e2e".into(),
                base_url: Some(openai.uri()),
                default_model: "gpt-4o".into(),
                enabled: true,
                timeout_ms: 30_000,
                max_tokens: Some(2_048),
            },
            Some("sk-dashboard-e2e"),
        )
        .await
        .unwrap();
    let chat: Value = server
        .client
        .post(format!("{}/api/v1/intelligence/chat", server.base_url))
        .header(server.auth_header().0, server.auth_header().1)
        .json(&json!({
            "provider": "openai",
            "model": "gpt-4o",
            "provider_id": provider.id,
            "title": "Application errors",
            "capability": "dashboard_authoring"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let chat_id = chat["id"].as_str().unwrap();
    assert_eq!(chat["capability"], "dashboard_authoring");

    let first_sse = server
        .client
        .post(format!(
            "{}/api/v1/intelligence/chat/{chat_id}/messages",
            server.base_url
        ))
        .header(server.auth_header().0, server.auth_header().1)
        .header("accept", "text/event-stream")
        .json(&json!({
            "content": "Create an app_errors error Dashboard from 2026-08-03T00:00:00Z to 2026-08-03T01:00:00Z",
            "capability": "dashboard_authoring",
            "execution_policy": "policy",
            "provider_id": provider.id
        }))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        first_sse.contains("event: done"),
        "chat did not finish: {first_sse}"
    );
    let discovered = tool_json_result(&first_sse, "e2e_streams");
    assert!(
        discovered["streams"]
            .as_array()
            .is_some_and(|streams| streams.iter().any(|stream| stream["name"] == "app_errors"))
    );
    let prepared = tool_json_result(&first_sse, "e2e_prepare");
    let draft_id = prepared["draft_id"].as_str().unwrap().to_string();
    let model_hash = prepared["model_hash"].as_str().unwrap().to_string();
    assert_eq!(
        prepared["preview_route"],
        format!("/ai/dashboard-drafts/{draft_id}")
    );
    *proposal_reference.lock().unwrap() = Some((draft_id.clone(), model_hash.clone()));

    let preview: Value = server
        .client
        .get(format!(
            "{}/api/v1/intelligence/dashboard-drafts/{draft_id}",
            server.base_url
        ))
        .header(server.auth_header().0, server.auth_header().1)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(preview["status"], "ready");
    assert_eq!(preview["model_hash"], model_hash);
    assert_eq!(preview["compiled_model"]["schemaVersion"], 2);

    let second_sse = server
        .client
        .post(format!(
            "{}/api/v1/intelligence/chat/{chat_id}/messages",
            server.base_url
        ))
        .header(server.auth_header().0, server.auth_header().1)
        .header("accept", "text/event-stream")
        .json(&json!({
            "content": "I reviewed the persisted preview; propose its creation.",
            "capability": "dashboard_authoring",
            "execution_policy": "policy",
            "provider_id": provider.id
        }))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let proposed = tool_json_result(&second_sse, "e2e_propose");
    let approval_id = proposed["approval"]["id"].as_str().unwrap();
    assert_eq!(proposed["approval"]["status"], "approved");
    assert_eq!(proposed["approval"]["required_approvals"], 0);

    let execution: Value = execute(&server, approval_id, "dashboard-chat-e2e")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(execution["status"], "succeeded");
    let dashboard_id = execution["verification"]["dashboard_id"].as_str().unwrap();
    let dashboard = server
        .client
        .get(format!(
            "{}/api/v1/dashboards/{dashboard_id}",
            server.base_url
        ))
        .header(server.auth_header().0, server.auth_header().1)
        .send()
        .await
        .unwrap();
    assert_eq!(dashboard.status(), 200);

    let replay: Value = execute(&server, approval_id, "dashboard-chat-e2e-replay")
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(replay["id"], execution["id"]);
    let dashboard_count =
        sqlx::query_scalar::<i64>("SELECT COUNT(*) FROM dashboards WHERE org_id = $1 AND id = $2")
            .bind(&server.root_org_id.0)
            .bind(dashboard_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(dashboard_count, 1);

    let requests = openai.received_requests().await.unwrap();
    assert_eq!(requests.len(), 5);
    let first_request: Value = requests[0].body_json().unwrap();
    assert_eq!(first_request["tool_choice"], "auto");
    assert!(first_request["tools"].as_array().is_some_and(|tools| {
        tools
            .iter()
            .any(|tool| tool["function"]["name"] == "prepare_dashboard")
    }));
    assert!(
        first_request["messages"]
            .as_array()
            .is_some_and(|messages| {
                messages.iter().any(|message| {
                    message["role"] == "system"
                        && message["content"]
                            .as_str()
                            .is_some_and(|content| content.contains("dashboard.authoring.v1"))
                })
            })
    );
    assert_eq!(provider_calls.load(Ordering::SeqCst), 5);
}
