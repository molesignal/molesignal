// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The draft postmortem of a status-page incident. Mole Agent writes drafts only: saving a draft
//! over a published postmortem would take it off the public page, so the repository refuses that
//! in the transaction that writes, and a published one is left to the people who revise it on the
//! status page.

use serde::Deserialize;
use serde_json::{Value, json};

use super::super::OperationOutcome;
use crate::{
    agent::model::ApprovalRequest,
    api::AppState,
    app::iam::IamContext,
    domain::status_page::PostmortemWrite,
    shared::{Error, Result, ids::Id},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftWrite {
    page_id: String,
    body: String,
}

pub(super) async fn draft(
    state: &AppState,
    ctx: &IamContext,
    approval: &ApprovalRequest,
) -> Result<OperationOutcome> {
    let parameters = parse(&approval.parameters)?;
    let page_id = Id(parameters.page_id);
    let event_id = Id(approval.target.clone());

    let incident = state
        .status_pages
        .save_postmortem(
            &ctx.org_id,
            &page_id,
            &event_id,
            &parameters.body,
            PostmortemWrite::DraftOnly,
            &ctx.user_id,
        )
        .await?;

    Ok(OperationOutcome {
        summary: "status page postmortem draft saved".into(),
        verification: json!({
            "verified": true,
            "page_id": page_id,
            "event_id": incident.id,
            "postmortem": incident.postmortem,
        }),
    })
}

fn parse(parameters: &Value) -> Result<DraftWrite> {
    serde_json::from_value(parameters.clone())
        .map_err(|error| Error::invalid(format!("invalid postmortem draft: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_draft_needs_its_page_and_body_and_nothing_else() {
        let draft = parse(&json!({"page_id": "page", "body": "# What happened"})).unwrap();
        assert_eq!(draft.page_id, "page");

        assert!(parse(&json!({"body": "# What happened"})).is_err());
        assert!(parse(&json!({"page_id": "page", "body": "x", "publish": true})).is_err());
    }
}
