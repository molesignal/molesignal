// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Running a log inspection now: the queued run the inspection page's "Run now" starts, requested
//! by whoever carries out the approved operation.

use serde_json::json;

use super::super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::AppState,
    app::iam::IamContext,
    shared::{Result, ids::Id},
};

pub(super) async fn run(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let run = state
        .platform
        .log_inspections
        .run_now(ctx, &Id(approval.target.clone()))
        .await?;

    Ok(OperationOutcome {
        summary: "log inspection run is queued".into(),
        verification: json!({
            "verified": true,
            "inspection_id": run.inspection_id,
            "run_id": run.id,
            "status": run.status,
        }),
    })
}
