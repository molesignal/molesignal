// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde_json::json;

use super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::AppState,
    app::iam::IamContext,
    domain::{status_page::StatusPageLifecycle, synthetics::MonitorLifecycle},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

mod alert_rules;
mod api_tokens;
mod content;
mod dashboard_panels;
mod dashboards;
mod folders;
mod functions;
mod saved_views;
mod search_jobs;
mod service_accounts;

pub(super) fn supports(action: &str) -> bool {
    matches!(
        action,
        "run_synthetic_monitor"
            | "pause_synthetic_monitor"
            | "resume_synthetic_monitor"
            | "archive_synthetic_monitor"
            | "archive_status_page"
            | "restore_status_page"
            | "pause_status_page_automation"
            | "resume_status_page_automation"
            | "retry_notification_delivery"
            | "acknowledge_notification_delivery"
            | "enable_scheduled_pipeline"
            | "disable_scheduled_pipeline"
            | "delete_scheduled_pipeline"
            | "trigger_alert_rule"
            | "create_alert_rule"
            | "update_alert_rule"
            | "delete_alert_rule"
            | "create_dashboard_model"
            | "update_dashboard"
            | "delete_dashboard"
            | "create_folder"
            | "update_folder"
            | "delete_folder"
            | "create_annotation"
            | "update_annotation"
            | "delete_annotation"
            | "add_dashboard_panel"
            | "update_dashboard_panel"
            | "move_dashboard_panel"
            | "delete_dashboard_panel"
            | "submit_search_job"
            | "cancel_search_job"
            | "retry_search_job"
            | "delete_search_job"
            | "create_saved_view"
            | "update_saved_view"
            | "delete_saved_view"
            | "create_function"
            | "update_function"
            | "delete_function"
            | "create_service_account"
            | "create_api_token"
            | "revoke_api_token"
            | "update_service_account"
            | "enable_service_account"
            | "disable_service_account"
            | "delete_service_account"
    )
}

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    match approval.action.as_str() {
        "run_synthetic_monitor" => run_synthetic(state, ctx, &approval.target).await,
        "pause_synthetic_monitor" => {
            synthetic_lifecycle(state, ctx, &approval.target, MonitorLifecycle::Paused).await
        }
        "resume_synthetic_monitor" => {
            synthetic_lifecycle(state, ctx, &approval.target, MonitorLifecycle::Active).await
        }
        "archive_synthetic_monitor" => {
            synthetic_lifecycle(state, ctx, &approval.target, MonitorLifecycle::Archived).await
        }
        "archive_status_page" => status_page_lifecycle(state, ctx, &approval.target, true).await,
        "restore_status_page" => status_page_lifecycle(state, ctx, &approval.target, false).await,
        "pause_status_page_automation" => {
            status_page_automation(state, ctx, &approval.target, true).await
        }
        "resume_status_page_automation" => {
            status_page_automation(state, ctx, &approval.target, false).await
        }
        "retry_notification_delivery" => retry_notification(state, ctx, &approval.target).await,
        "acknowledge_notification_delivery" => {
            acknowledge_notification(state, ctx, &approval.target).await
        }
        "enable_scheduled_pipeline" => {
            scheduled_pipeline_lifecycle(state, ctx, &approval.target, true).await
        }
        "disable_scheduled_pipeline" => {
            scheduled_pipeline_lifecycle(state, ctx, &approval.target, false).await
        }
        "delete_scheduled_pipeline" => {
            delete_scheduled_pipeline(state, ctx, &approval.target).await
        }
        "trigger_alert_rule" | "create_alert_rule" | "update_alert_rule" | "delete_alert_rule" => {
            alert_rules::execute(state, ctx, approval).await
        }
        "create_dashboard_model" | "update_dashboard" | "delete_dashboard" => {
            dashboards::execute(state, ctx, approval).await
        }
        "create_folder" | "update_folder" | "delete_folder" => {
            folders::execute(state, ctx, approval).await
        }
        "create_annotation" | "update_annotation" | "delete_annotation" => {
            content::execute(state, ctx, approval).await
        }
        "add_dashboard_panel"
        | "update_dashboard_panel"
        | "move_dashboard_panel"
        | "delete_dashboard_panel" => dashboard_panels::execute(state, ctx, approval).await,
        "submit_search_job" | "cancel_search_job" | "retry_search_job" | "delete_search_job" => {
            search_jobs::execute(state, ctx, approval).await
        }
        "create_saved_view" | "update_saved_view" | "delete_saved_view" => {
            saved_views::execute(state, ctx, approval).await
        }
        "create_function" | "update_function" | "delete_function" => {
            functions::execute(state, ctx, approval).await
        }
        "create_service_account"
        | "update_service_account"
        | "enable_service_account"
        | "disable_service_account"
        | "delete_service_account" => service_accounts::execute(state, ctx, approval).await,
        "create_api_token" | "revoke_api_token" => api_tokens::execute(state, ctx, approval).await,
        other => Err(Error::invalid(format!(
            "operation '{}' is not registered",
            other
        ))),
    }
}

async fn run_synthetic(
    state: &AppState,
    ctx: &IamContext,
    target: &str,
) -> Result<OperationOutcome> {
    let monitor_id = Id(target.to_string());
    state
        .synthetics
        .get_monitor(&ctx.org_id, &monitor_id)
        .await?;
    let tasks = state
        .synthetics
        .run_monitor(&ctx.org_id, &monitor_id)
        .await?;
    Ok(OperationOutcome {
        summary: format!("queued {} synthetic probe tasks", tasks.len()),
        verification: json!({
            "verified": true,
            "monitor_id": monitor_id,
            "task_count": tasks.len(),
            "tasks": tasks,
        }),
    })
}

async fn synthetic_lifecycle(
    state: &AppState,
    ctx: &IamContext,
    target: &str,
    lifecycle: MonitorLifecycle,
) -> Result<OperationOutcome> {
    let monitor_id = Id(target.to_string());
    let current = state
        .synthetics
        .get_monitor(&ctx.org_id, &monitor_id)
        .await?;
    let monitor = if current.lifecycle == lifecycle {
        current
    } else {
        state
            .synthetics
            .set_lifecycle(&ctx.org_id, &monitor_id, lifecycle)
            .await?
    };
    Ok(OperationOutcome {
        summary: format!("synthetic monitor lifecycle is {}", lifecycle.as_str()),
        verification: json!({"verified": true, "monitor": monitor}),
    })
}

async fn status_page_lifecycle(
    state: &AppState,
    ctx: &IamContext,
    target: &str,
    archive: bool,
) -> Result<OperationOutcome> {
    let page_id = Id(target.to_string());
    let current = state.status_pages.get_page(&ctx.org_id, &page_id).await?;
    let desired = if archive {
        StatusPageLifecycle::Archived
    } else {
        StatusPageLifecycle::Active
    };
    let page = if current.lifecycle == desired {
        current
    } else if archive {
        state
            .status_pages
            .archive_page(&ctx.org_id, &page_id)
            .await?
    } else {
        state
            .status_pages
            .restore_page(&ctx.org_id, &page_id)
            .await?
    };
    Ok(OperationOutcome {
        summary: if archive {
            "status page is archived".into()
        } else {
            "status page is active".into()
        },
        verification: json!({"verified": true, "status_page": page}),
    })
}

async fn status_page_automation(
    state: &AppState,
    ctx: &IamContext,
    target: &str,
    paused: bool,
) -> Result<OperationOutcome> {
    let page_id = Id(target.to_string());
    let settings = state
        .status_pages
        .set_automation_paused(&ctx.org_id, &page_id, &ctx.user_id, paused)
        .await?;
    Ok(OperationOutcome {
        summary: if paused {
            "status page automation is paused".into()
        } else {
            "status page automation is active".into()
        },
        verification: json!({"verified": true, "settings": settings}),
    })
}

async fn retry_notification(
    state: &AppState,
    ctx: &IamContext,
    target: &str,
) -> Result<OperationOutcome> {
    let outcome = state
        .alerting
        .notify_engine
        .retry_delivery(&ctx.org_id, &Id(target.to_string()))
        .await?;
    Ok(OperationOutcome {
        summary: "notification delivery retry completed".into(),
        verification: json!({"verified": true, "outcome": outcome}),
    })
}

async fn acknowledge_notification(
    state: &AppState,
    ctx: &IamContext,
    target: &str,
) -> Result<OperationOutcome> {
    let delivery = state
        .alerting
        .notify_engine
        .acknowledge_delivery(&ctx.org_id, &Id(target.to_string()), TimestampMicros::now())
        .await?;
    Ok(OperationOutcome {
        summary: "notification delivery acknowledged".into(),
        verification: json!({"verified": true, "delivery": delivery}),
    })
}

async fn scheduled_pipeline_lifecycle(
    state: &AppState,
    ctx: &IamContext,
    target: &str,
    enabled: bool,
) -> Result<OperationOutcome> {
    let pipeline_id = Id(target.to_string());
    let mut pipeline = state
        .storage
        .scheduled_pipelines
        .get(&ctx.org_id, &pipeline_id)
        .await?;
    if pipeline.enabled != enabled {
        pipeline.enabled = enabled;
        pipeline.updated_at = TimestampMicros::now();
        pipeline = state.storage.scheduled_pipelines.update(pipeline).await?;
    }
    Ok(OperationOutcome {
        summary: if enabled {
            "scheduled pipeline is enabled".into()
        } else {
            "scheduled pipeline is disabled".into()
        },
        verification: json!({"verified": true, "pipeline": pipeline}),
    })
}

async fn delete_scheduled_pipeline(
    state: &AppState,
    ctx: &IamContext,
    target: &str,
) -> Result<OperationOutcome> {
    let pipeline_id = Id(target.to_string());
    state
        .storage
        .scheduled_pipelines
        .get(&ctx.org_id, &pipeline_id)
        .await?;
    state
        .storage
        .scheduled_pipelines
        .delete(&ctx.org_id, &pipeline_id)
        .await?;
    Ok(OperationOutcome {
        summary: "scheduled pipeline deleted".into(),
        verification: json!({
            "verified": true,
            "pipeline_id": pipeline_id,
            "deleted": true,
        }),
    })
}
