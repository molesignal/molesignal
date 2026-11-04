// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 升级策略、排班、路由规则的导入导出（见 [`crate::app::alerting::oncall_transfer`]）：
//!
//! - `GET /api/v1/alerts/escalations/export?ids=`、`POST /api/v1/alerts/escalations/import`
//!   （`alerts.escalations.read` / `alerts.escalations.manage`）；
//! - `GET /api/v1/schedules/export?ids=`、`POST /api/v1/schedules/import`（`schedules.read` /
//!   `schedules.create`）；
//! - `GET /api/v1/alerts/routing/rules/export`、`POST /api/v1/alerts/routing/rules/import`
//!   （`alerts.routing.read` / `alerts.routing.manage`）：包里的规则按包里的先后排在最前面，其余的保持原来的先后。
//!
//! 导入的请求体是 `{content, dry_run = true, create_only = false}`。排班的两个接口见 [`schedules`]。

use axum::{
    Extension, Json, Router,
    extract::{DefaultBodyLimit, Query, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    api::{
        AppState,
        http::{federation::emit_cud, routes::activity_audit},
    },
    app::{
        alerting::{
            oncall_transfer::{
                Directory, Existing, Names, POLICY_FORMAT, ROUTING_FORMAT, TransferItem,
                TransferReport, Write, plan, portable_of, read_bundle,
            },
            routing::RuleWrite,
            rule_transfer::ImportAction,
        },
        iam::IamContext,
    },
    domain::{
        alerting::{
            escalation::{
                DEFAULT_POLICY_ID, EscalationPolicy, EscalationStep, validate_ack_retrigger,
            },
            urgency::UrgencyRule,
        },
        federation::{CudAction, ResourceKind},
        iam::permission,
    },
    shared::{Error, Result, ids::Id},
};

mod schedules;

/// 一个 4 MiB 的文件写进 JSON 字符串后的大小。
const MAX_REQUEST_BYTES: usize = 6 * 1024 * 1024;

const POLICY_DROP: [&str; 2] = ["id", "orgId"];
const ROUTING_DROP: [&str; 6] = [
    "id",
    "orgId",
    "position",
    "createdBy",
    "createdAt",
    "updatedAt",
];

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/escalations/export", get(export_policies))
        .route("/alerts/escalations/import", post(import_policies))
        .route("/schedules/export", get(schedules::export_schedules))
        .route(
            "/schedules/import",
            post(schedules::import_schedules).layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES)),
        )
        .route("/alerts/routing/rules/export", get(export_routing))
        .route("/alerts/routing/rules/import", post(import_routing))
}

#[derive(Debug, Default, Deserialize)]
struct ExportQuery {
    /// 只导出这些（逗号分隔）；不给就是全部。
    #[serde(default)]
    ids: Option<String>,
}

impl ExportQuery {
    fn keeps(&self, id: &Id) -> bool {
        self.ids
            .as_deref()
            .filter(|ids| !ids.trim().is_empty())
            .is_none_or(|ids| ids.split(',').any(|wanted| wanted.trim() == id.0))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportRequest {
    content: String,
    #[serde(default = "dry_run_by_default")]
    dry_run: bool,
    #[serde(default)]
    create_only: bool,
}

fn dry_run_by_default() -> bool {
    true
}

/// 组织里能被引用的东西的名字。
async fn directory(state: &AppState, org_id: &Id) -> Result<Directory> {
    let members = state
        .iam
        .service
        .iam_memberships
        .list_for_org(org_id)
        .await?;
    let user_ids: Vec<Id> = members.into_iter().map(|member| member.user_id).collect();
    let users = state.iam.service.users.get_many(&user_ids).await?;
    let teams = state.iam.teams.list(org_id).await?;
    let schedules = state.alerting.service.list_schedules(org_id).await?;
    let policies = state.alerting.service.list_policies(org_id).await?;
    let calendars = state.alerting.holidays.list(org_id).await?;

    // 路由规则和事件里的 `default` 是别名：组织没有叫这个名字的策略时，按别名原样进出。
    let alias = (!policies
        .iter()
        .any(|policy| policy.name.trim().eq_ignore_ascii_case(DEFAULT_POLICY_ID)))
    .then(|| {
        (
            Id::from_string(DEFAULT_POLICY_ID),
            DEFAULT_POLICY_ID.to_string(),
        )
    });

    Ok(Directory {
        users: Names::new(users.into_iter().map(|user| (user.id, user.email)), true),
        teams: Names::new(teams.into_iter().map(|team| (team.id, team.name)), false),
        schedules: Names::new(
            schedules.into_iter().map(|item| (item.id, item.name)),
            false,
        ),
        policies: Names::new(
            policies
                .into_iter()
                .map(|item| (item.id, item.name))
                .chain(alias),
            false,
        ),
        calendars: Names::new(
            calendars.into_iter().map(|item| (item.id, item.name)),
            false,
        ),
    })
}

fn report(format: &'static str, items: Vec<TransferItem>, applied: bool) -> Json<TransferReport> {
    Json(TransferReport {
        format,
        items,
        applied,
        findings: Vec::new(),
    })
}

fn failed(items: &[TransferItem]) -> bool {
    items.iter().any(|item| item.action == ImportAction::Error)
}

// ---- escalation policies ----

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PolicyWrite {
    name: String,
    #[serde(default)]
    steps: Vec<EscalationStep>,
    #[serde(default)]
    repeat: bool,
    #[serde(default = "one")]
    max_loops: u32,
    #[serde(default)]
    ack_retrigger_secs: Option<u32>,
    #[serde(default)]
    urgency_rule: UrgencyRule,
    #[serde(default)]
    escalate_low_urgency: bool,
}

fn one() -> u32 {
    1
}

#[permission("alerts.escalations.read")]
async fn export_policies(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ExportQuery>,
) -> Result<Json<Value>> {
    let directory = directory(&state, &ctx.org_id).await?;
    let policies: Vec<Value> = state
        .alerting
        .service
        .list_policies(&ctx.org_id)
        .await?
        .iter()
        .filter(|policy| query.keeps(&policy.id))
        .map(|policy| portable_of(policy, &POLICY_DROP, &directory))
        .collect();

    Ok(Json(
        json!({ "format": POLICY_FORMAT, "policies": policies }),
    ))
}

#[permission("alerts.escalations.manage")]
async fn import_policies(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<ImportRequest>,
) -> Result<Json<TransferReport>> {
    let incoming = read_bundle(&request.content, POLICY_FORMAT, "policies")?;
    let directory = directory(&state, &ctx.org_id).await?;
    let levels = state.alerting.importance.levels(&ctx.org_id).await;
    let current = state.alerting.service.list_policies(&ctx.org_id).await?;
    let existing: Vec<Existing> = current
        .iter()
        .map(|policy| Existing {
            id: policy.id.clone(),
            name: policy.name.clone(),
            portable: portable_of(policy, &POLICY_DROP, &directory),
        })
        .collect();

    let (mut items, writes) = plan(
        incoming,
        &existing,
        &directory,
        request.create_only,
        |value, id| {
            let write: PolicyWrite = serde_json::from_value(value)
                .map_err(|error| Error::invalid(format!("not an escalation policy: {error}")))?;

            if write.name.trim().is_empty() {
                return Err(Error::invalid("name cannot be empty"));
            }

            validate_ack_retrigger(write.ack_retrigger_secs)?;
            write.urgency_rule.validate()?;
            write.urgency_rule.check_levels(&levels)?;

            if let Some(calendar) = write.urgency_rule.calendar() {
                calendar.check(&ctx.org_id)?;
            }

            Ok(EscalationPolicy {
                id: id.cloned().unwrap_or_default(),
                org_id: ctx.org_id.clone(),
                name: write.name.trim().to_string(),
                steps: write.steps,
                repeat: write.repeat,
                max_loops: write.max_loops.max(1),
                ack_retrigger_secs: write.ack_retrigger_secs,
                urgency_rule: write.urgency_rule,
                escalate_low_urgency: write.escalate_low_urgency,
            })
        },
        |policy| portable_of(policy, &POLICY_DROP, &directory),
    );

    if request.dry_run || failed(&items) {
        return Ok(report(POLICY_FORMAT, items, false));
    }

    for Write {
        existing,
        value,
        item,
    } in writes
    {
        let (action, saved) = match existing {
            Some(_) => (
                CudAction::Updated,
                state.alerting.service.update_policy(value).await?,
            ),
            None => (
                CudAction::Created,
                state
                    .alerting
                    .service
                    .create_policy(EscalationPolicy {
                        id: Id::new(),
                        ..value
                    })
                    .await?,
            ),
        };

        emit_cud(
            &state,
            &ctx.org_id,
            ResourceKind::EscalationPolicy,
            action,
            &saved.id.0,
            &saved,
        )
        .await;

        items[item].id = Some(saved.id);
    }

    audit(
        &state,
        &ctx,
        "escalation_policies.imported",
        "escalation_policy",
        &items,
    )
    .await;
    Ok(report(POLICY_FORMAT, items, true))
}

// ---- routing rules ----

#[permission("alerts.routing.read")]
async fn export_routing(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ExportQuery>,
) -> Result<Json<Value>> {
    let directory = directory(&state, &ctx.org_id).await?;
    let mut rules = state.alerting.routing.rules(&ctx.org_id).await?;

    rules.sort_by_key(|rule| rule.position);

    let rules: Vec<Value> = rules
        .iter()
        .filter(|rule| query.keeps(&rule.id))
        .map(|rule| portable_of(rule, &ROUTING_DROP, &directory))
        .collect();

    Ok(Json(json!({ "format": ROUTING_FORMAT, "rules": rules })))
}

#[permission("alerts.routing.manage")]
async fn import_routing(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<ImportRequest>,
) -> Result<Json<TransferReport>> {
    let incoming = read_bundle(&request.content, ROUTING_FORMAT, "rules")?;
    let directory = directory(&state, &ctx.org_id).await?;
    let levels = state.alerting.importance.levels(&ctx.org_id).await;
    let mut current = state.alerting.routing.rules(&ctx.org_id).await?;

    current.sort_by_key(|rule| rule.position);

    let existing: Vec<Existing> = current
        .iter()
        .map(|rule| Existing {
            id: rule.id.clone(),
            name: rule.name.clone(),
            portable: portable_of(rule, &ROUTING_DROP, &directory),
        })
        .collect();

    // 预演时一条条照保存那样查（目标、日历、条件），这几步要读库，先收齐再查。
    let (mut items, writes) = plan(
        incoming,
        &existing,
        &directory,
        request.create_only,
        |value, _| {
            let write: RuleWrite = serde_json::from_value(value)
                .map_err(|error| Error::invalid(format!("not a routing rule: {error}")))?;

            for priority in &write.priorities {
                levels.check_priority(*priority)?;
            }

            Ok(write)
        },
        |write| portable_of(&portable_write(write), &[], &directory),
    );

    for (item, write) in items
        .iter_mut()
        .filter(|item| matches!(item.action, ImportAction::Create | ImportAction::Update))
        .zip(&writes)
    {
        if let Err(error) = state
            .alerting
            .routing
            .check_write(&ctx.org_id, &write.value)
            .await
        {
            item.action = ImportAction::Error;
            item.error = Some(error.to_string());
        }
    }

    if request.dry_run || failed(&items) {
        return Ok(report(ROUTING_FORMAT, items, false));
    }

    for Write {
        existing,
        value,
        item,
    } in writes
    {
        let saved = match existing {
            Some(id) => {
                state
                    .alerting
                    .routing
                    .update_rule(&ctx.org_id, &id, value)
                    .await?
            }
            None => {
                state
                    .alerting
                    .routing
                    .create_rule(&ctx.org_id, &ctx.user_id, value)
                    .await?
            }
        };

        items[item].id = Some(saved.id);
    }

    // 包里的先后：每一项的 id（写过的是写后的，不变的是已有的）排在最前面；其余保持原来的先后。
    let mut order: Vec<Id> = items.iter().filter_map(|item| item.id.clone()).collect();

    for rule in &current {
        if !order.contains(&rule.id) {
            order.push(rule.id.clone());
        }
    }

    state.alerting.routing.reorder(&ctx.org_id, &order).await?;
    audit(
        &state,
        &ctx,
        "routing_rules.imported",
        "routing_rule",
        &items,
    )
    .await;
    Ok(report(ROUTING_FORMAT, items, true))
}

/// 要写的路由规则换成和导出一样的样子，好和已有的比。
fn portable_write(write: &RuleWrite) -> Value {
    let mut value = json!({
        "name": write.name.trim(),
        "enabled": write.enabled,
        "conditions": write.conditions,
        "minSeverity": write.min_severity,
        "origins": write.origins,
        "priorities": write.priorities,
        "target": write.target,
    });

    if let (Some(time), Value::Object(object)) = (&write.time, &mut value) {
        object.insert("time".into(), json!(time));
    }

    if let Value::Object(object) = &mut value
        && write.priorities.is_empty()
    {
        object.remove("priorities");
    }

    value
}

async fn audit(
    state: &AppState,
    ctx: &IamContext,
    action: &str,
    kind: &str,
    items: &[TransferItem],
) {
    let writes = items
        .iter()
        .filter(|item| matches!(item.action, ImportAction::Create | ImportAction::Update))
        .count();

    activity_audit::record(state, ctx, action, kind, "", json!({ "written": writes })).await;
}
