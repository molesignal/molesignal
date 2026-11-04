// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 排班的导入导出：`GET /api/v1/schedules/export?ids=`、`POST /api/v1/schedules/import`
//! （`schedules.read` / `schedules.create`）。导入除了自家的包，也接受 PagerDuty、Opsgenie 的 API
//! 响应（见 [`crate::app::alerting::oncall_transfer::foreign`]）。

use axum::{
    Extension, Json,
    extract::{Query, State},
};
use serde_json::{Value, json};

use super::{ExportQuery, ImportRequest, audit, directory, failed};
use crate::{
    api::{AppState, http::federation::emit_cud},
    app::{
        alerting::oncall_transfer::{
            Existing, Finding, MAX_IMPORT_ITEMS, SCHEDULE_FORMAT, TransferReport, Write,
            bundle_items, foreign, plan, portable_of, read_content,
            schedules::{SCHEDULE_DROP, ScheduleScope, schedule_from_item},
        },
        iam::IamContext,
    },
    domain::{
        alerting::schedule::Schedule,
        federation::{CudAction, ResourceKind},
        iam::permission,
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[permission("schedules.read")]
pub(super) async fn export_schedules(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<ExportQuery>,
) -> Result<Json<Value>> {
    let directory = directory(&state, &ctx.org_id).await?;
    let schedules: Vec<Value> = state
        .alerting
        .service
        .list_schedules(&ctx.org_id)
        .await?
        .iter()
        .filter(|schedule| query.keeps(&schedule.id))
        .map(|schedule| portable_of(schedule, &SCHEDULE_DROP, &directory))
        .collect();

    Ok(Json(
        json!({ "format": SCHEDULE_FORMAT, "schedules": schedules }),
    ))
}

#[permission("schedules.create")]
pub(super) async fn import_schedules(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<ImportRequest>,
) -> Result<Json<TransferReport>> {
    let bundle = read_content(&request.content)?;
    let directory = directory(&state, &ctx.org_id).await?;
    let now = TimestampMicros::now();
    let source = foreign::read(&bundle, &directory, now);

    let (format, incoming) = match &source {
        Some(source) => {
            if source.schedules.len() + source.left_out.len() > MAX_IMPORT_ITEMS {
                return Err(Error::invalid(format!(
                    "at most {MAX_IMPORT_ITEMS} items in one import"
                )));
            }

            (source.format, source.incoming())
        }
        None if bundle.get("format").is_none() => {
            return Err(Error::invalid(format!(
                "this is neither a `{SCHEDULE_FORMAT}` bundle nor PagerDuty or Opsgenie schedules"
            )));
        }
        None => (
            SCHEDULE_FORMAT,
            bundle_items(&bundle, SCHEDULE_FORMAT, "schedules")?,
        ),
    };

    let current = state.alerting.service.list_schedules(&ctx.org_id).await?;
    let existing: Vec<Existing> = current
        .iter()
        .map(|schedule| Existing {
            id: schedule.id.clone(),
            name: schedule.name.clone(),
            portable: portable_of(schedule, &SCHEDULE_DROP, &directory),
        })
        .collect();
    let scope = ScheduleScope {
        org_id: &ctx.org_id,
        user_id: &ctx.user_id,
        now,
    };

    let (mut items, writes) = plan(
        incoming,
        &existing,
        &directory,
        request.create_only,
        |value, id| {
            let before = id.and_then(|id| current.iter().find(|schedule| schedule.id == *id));

            schedule_from_item(value, id, before, &scope)
        },
        |schedule| portable_of(schedule, &SCHEDULE_DROP, &directory),
    );

    let findings: Vec<Finding> = source
        .map(|source| source.annotate(&mut items))
        .unwrap_or_default();

    if request.dry_run || failed(&items) {
        return Ok(Json(TransferReport {
            format,
            items,
            applied: false,
            findings,
        }));
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
                state.alerting.service.update_schedule(value).await?,
            ),
            None => (
                CudAction::Created,
                state
                    .alerting
                    .service
                    .create_schedule(Schedule {
                        id: Id::new(),
                        ..value
                    })
                    .await?,
            ),
        };

        emit_cud(
            &state,
            &ctx.org_id,
            ResourceKind::Schedule,
            action,
            &saved.id.0,
            &saved,
        )
        .await;

        items[item].id = Some(saved.id);
    }

    audit(&state, &ctx, "schedules.imported", "schedule", &items).await;
    Ok(Json(TransferReport {
        format,
        items,
        applied: true,
        findings,
    }))
}
