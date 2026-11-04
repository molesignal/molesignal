// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 修复动作批准后执行：审批的目标是事件，参数里有动作与填好的参数，执行与验证由
//! `RemediationService` 做，以批准执行的人的身份记下。

use std::collections::BTreeMap;

use serde::Deserialize;

use super::approval_execution::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::AppState,
    app::iam::IamContext,
    domain::alerting::remediation::RemediationRisk,
    shared::{Error, Result, ids::Id},
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Params {
    action_id: Id,
    #[serde(default)]
    inputs: BTreeMap<String, String>,
}

/// 审批里这个操作的名字。
pub(crate) const ACTION: &str = "run_remediation_action";

pub(super) async fn execute(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let service = &state.alerting.remediation;
    let params: Params = serde_json::from_value(approval.parameters.clone())
        .map_err(|error| Error::invalid(format!("the remediation action's parameters: {error}")))?;
    let incident = state
        .alerting
        .service
        .get_incident(&Id(approval.target.clone()))
        .await?;

    if incident.org_id != ctx.org_id {
        return Err(Error::forbidden("alert belongs to another organization"));
    }

    let (action, inputs) = service
        .prepare(&incident, &params.action_id, &params.inputs)
        .await?;

    // 修复动作一律审批：L2 至少一人、L3 至少两人批准过，不管提议时用的是什么模式。
    let needed = match action.risk {
        RemediationRisk::L2 => 1,
        RemediationRisk::L3 => 2,
    };

    if approval.required_approvals < needed {
        return Err(Error::forbidden(format!(
            "this remediation action needs {needed} approval(s) before it runs"
        )));
    }

    // Mole Agent 提议的还没有执行记录：这里补上。
    if service
        .repository()
        .find_run_by_approval(&ctx.org_id, &approval.id)
        .await?
        .is_none()
    {
        service
            .record_request(
                &incident,
                &action,
                inputs,
                approval.id.clone(),
                &approval.requested_by,
            )
            .await?;
    }

    let (summary, verification) = state
        .alerting
        .remediation
        .execute(&ctx.org_id, &approval.id, ctx.principal_id())
        .await?;

    Ok(OperationOutcome {
        summary,
        verification,
    })
}
