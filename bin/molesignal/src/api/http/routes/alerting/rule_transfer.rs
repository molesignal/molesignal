// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 规则的导入导出：
//!
//! - `GET /api/v1/alerts/rules/export?ids=a,b`（`alerts.rules.read`）：导出包，`ids` 空着是全部规则。
//! - `POST /api/v1/alerts/rules/import`（`alerts.rules.manage`）：`{"content": "...", "dryRun": true,
//!   "createOnly": false}`。`content` 是导出包（JSON 或 YAML）或 Prometheus 的告警规则文件；默认
//!   只预演，`dryRun: false` 才写。每一条按名称对上已有规则，整包都说得通才一次写进去。

use std::collections::{BTreeMap, HashMap};

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::json;

use super::{
    check_rum_application, normalize_anomaly,
    rule_kinds::{check_composite, check_home_cluster},
};
use crate::{
    api::{
        AppState,
        http::{federation::emit_cud, routes::activity_audit},
    },
    app::{
        alerting::rule_transfer::{
            self, ImportAction, ImportItem, ImportReport,
            bundle::{BUNDLE_FORMAT, PortableRule, RuleBundle, changed_fields, portable},
        },
        iam::IamContext,
    },
    domain::{
        alerting::{
            escalation::{DEFAULT_POLICY_ID, EscalationPolicy},
            rule::{AlertRule, RuleState},
        },
        federation::{CudAction, ResourceKind},
        iam::permission,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/alerts/rules/export", get(export))
        .route("/alerts/rules/import", post(import))
}

#[derive(Debug, Default, Deserialize)]
struct ExportQuery {
    /// 逗号分开的规则 id；空着是全部。
    #[serde(default)]
    ids: Option<String>,
}

#[permission("alerts.rules.read")]
async fn export(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ExportQuery>,
) -> Result<Json<RuleBundle>> {
    let rules = state.alerting.service.list_rules(&ctx.org_id).await?;
    let policies = state.alerting.service.list_policies(&ctx.org_id).await?;

    let names: BTreeMap<String, String> = rules
        .iter()
        .map(|rule| (rule.id.0.clone(), rule.name.clone()))
        .collect();

    let wanted: Vec<&str> = query
        .ids
        .as_deref()
        .map(|ids| {
            ids.split(',')
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .collect()
        })
        .unwrap_or_default();

    let mut exported: Vec<PortableRule> = rules
        .iter()
        .filter(|rule| wanted.is_empty() || wanted.contains(&rule.id.as_str()))
        .map(|rule| portable(rule, &policies, &names))
        .collect();
    exported.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(Json(RuleBundle {
        format: BUNDLE_FORMAT.into(),
        exported_at: Some(chrono::Utc::now().to_rfc3339()),
        rules: exported,
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportReq {
    content: String,
    #[serde(default = "dry_run_by_default")]
    dry_run: bool,
    /// 只新建：已有的同名规则不动。
    #[serde(default)]
    create_only: bool,
}

fn dry_run_by_default() -> bool {
    true
}

#[permission("alerts.rules.manage")]
async fn import(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(req): Json<ImportReq>,
) -> Result<Json<ImportReport>> {
    let (format, incoming, notes) = rule_transfer::parse(&req.content)?;

    let existing = state.alerting.service.list_rules(&ctx.org_id).await?;
    let policies = state.alerting.service.list_policies(&ctx.org_id).await?;
    let now = TimestampMicros::now();

    // 同名的已有规则，和这一包里每个名称最后落到哪条规则上。
    let mut by_name: HashMap<&str, Vec<&AlertRule>> = HashMap::new();

    for rule in &existing {
        by_name.entry(rule.name.as_str()).or_default().push(rule);
    }

    let mut planned: HashMap<String, Id> = existing
        .iter()
        .map(|rule| (rule.name.clone(), rule.id.clone()))
        .collect();
    let mut seen: HashMap<String, usize> = HashMap::new();

    for rule in &incoming {
        *seen.entry(rule.name.clone()).or_default() += 1;

        if by_name.get(rule.name.as_str()).is_none_or(Vec::is_empty) {
            planned.insert(rule.name.clone(), Id::new());
        }
    }

    let names: BTreeMap<String, String> = existing
        .iter()
        .map(|rule| (rule.id.0.clone(), rule.name.clone()))
        .collect();

    let lookups = Lookups {
        planned: &planned,
        existing_names: &names,
        policies: &policies,
    };

    let mut items = Vec::new();
    let mut creates = Vec::new();
    let mut updates = Vec::new();

    for rule in incoming {
        let name = rule.name.trim().to_string();
        let matches = by_name.get(name.as_str()).cloned().unwrap_or_default();

        if name.is_empty() {
            items.push(ImportItem::failed(&rule.name, "a rule needs a name"));
            continue;
        }

        if seen.get(&name).copied().unwrap_or_default() > 1 {
            items.push(ImportItem::failed(
                &name,
                "the import names this rule more than once",
            ));
            continue;
        }

        if matches.len() > 1 {
            items.push(ImportItem::failed(
                &name,
                format!(
                    "{} rules already have this name; rename them first",
                    matches.len()
                ),
            ));
            continue;
        }

        let current = matches.first().copied();

        if req.create_only
            && let Some(current) = current
        {
            items.push(ImportItem {
                name,
                action: ImportAction::Skip,
                changes: Vec::new(),
                error: None,
                rule_id: Some(current.id.clone()),
            });
            continue;
        }

        let built = built_rule(&state, &ctx.org_id, rule, current, &lookups, now).await;

        let mut built = match built {
            Ok(built) => built,
            Err(error) => {
                items.push(ImportItem::failed(&name, error.to_string()));
                continue;
            }
        };

        if let Err(error) = state.alerting.service.check_rule(&mut built).await {
            items.push(ImportItem::failed(&name, error.to_string()));
            continue;
        }

        match current {
            None => {
                items.push(ImportItem {
                    name,
                    action: ImportAction::Create,
                    changes: Vec::new(),
                    error: None,
                    rule_id: Some(built.id.clone()),
                });
                creates.push(built);
            }
            Some(current) => {
                let changes = changed_fields(
                    &portable(current, &policies, &names),
                    &portable(&built, &policies, &names),
                );

                let action = if changes.is_empty() {
                    ImportAction::Unchanged
                } else {
                    ImportAction::Update
                };

                items.push(ImportItem {
                    name,
                    action,
                    changes,
                    error: None,
                    rule_id: Some(built.id.clone()),
                });

                if action == ImportAction::Update {
                    updates.push(built);
                }
            }
        }
    }

    let failed = items.iter().any(|item| item.action == ImportAction::Error);
    let writes = creates.len() + updates.len();

    if req.dry_run || failed || writes == 0 {
        return Ok(Json(ImportReport {
            format,
            items,
            notes,
            applied: false,
        }));
    }

    let written: Vec<(CudAction, AlertRule)> = creates
        .iter()
        .map(|rule| (CudAction::Created, rule.clone()))
        .chain(
            updates
                .iter()
                .map(|rule| (CudAction::Updated, rule.clone())),
        )
        .collect();

    state.alerting.service.save_rules(creates, updates).await?;

    for (action, rule) in &written {
        emit_cud(
            &state,
            &ctx.org_id,
            ResourceKind::AlertRule,
            *action,
            &rule.id.0,
            rule,
        )
        .await;
    }

    activity_audit::record(
        &state,
        &ctx,
        "alert_rules.imported",
        "alert_rule",
        "",
        json!({ "format": format, "written": writes }),
    )
    .await;

    Ok(Json(ImportReport {
        format,
        items,
        notes,
        applied: true,
    }))
}

/// 一条要导入的定义换成规则：升级策略和子规则从名称换回 id，跑一遍保存时的检查。
/// 导入时按名称找东西用的：每个名称落到哪条规则上，已有规则的 id → 名称，组织的升级策略。
struct Lookups<'a> {
    planned: &'a HashMap<String, Id>,
    existing_names: &'a BTreeMap<String, String>,
    policies: &'a [EscalationPolicy],
}

async fn built_rule(
    state: &AppState,
    org_id: &Id,
    rule: PortableRule,
    current: Option<&AlertRule>,
    lookups: &Lookups<'_>,
    now: TimestampMicros,
) -> Result<AlertRule> {
    let Lookups {
        planned,
        existing_names,
        policies,
    } = lookups;

    let escalation_policy_id = match rule.escalation_policy.as_deref() {
        None | Some("") | Some(DEFAULT_POLICY_ID) => Id::from_string(DEFAULT_POLICY_ID),
        Some(name) => policies
            .iter()
            .find(|policy| policy.name == name)
            .map(|policy| policy.id.clone())
            .ok_or_else(|| Error::invalid(format!("no escalation policy is named `{name}`")))?,
    };

    let mut query = rule.query;
    let id = current
        .map(|current| current.id.clone())
        .or_else(|| planned.get(rule.name.trim()).cloned())
        .unwrap_or_default();

    if let Some(composite) = &mut query.composite {
        for child in composite.rules.values_mut() {
            let resolved = planned.get(child.as_str()).cloned().ok_or_else(|| {
                Error::invalid(format!(
                    "the composite rule names `{child}`, which is no rule"
                ))
            })?;
            *child = resolved;
        }
    }

    let kind = query.rule_kind(rule.kind);
    query.normalize(kind)?;
    super::importance::check_trigger(state, org_id, &rule.trigger).await?;
    check_rum_application(state, org_id, &query).await?;
    check_home_cluster(state, &query).await?;

    // 子规则都已经在库里时照保存时那样查；有的是这一包新建的，换 id 时已经对上了名称。
    let children_exist = query.composite.as_ref().is_none_or(|composite| {
        composite
            .rules
            .values()
            .all(|child| existing_names.contains_key(child.as_str()))
    });

    if children_exist {
        check_composite(state, org_id, kind, &query, Some(&id)).await?;
    }

    Ok(AlertRule {
        id,
        org_id: org_id.clone(),
        name: rule.name.trim().to_string(),
        description: rule.description,
        enabled: rule.enabled,
        kind,
        anomaly_params: normalize_anomaly(kind, rule.anomaly_params)?,
        query,
        trigger: rule.trigger,
        thresholds: rule.thresholds,
        severity: rule.severity,
        escalation_policy_id,
        labels: rule.labels,
        annotations: rule.annotations,
        last_eval_at: current.and_then(|current| current.last_eval_at),
        last_state: current
            .map(|current| current.last_state)
            .unwrap_or(RuleState::Healthy),
        created_at: current.map_or(now, |current| current.created_at),
        updated_at: now,
    })
}
