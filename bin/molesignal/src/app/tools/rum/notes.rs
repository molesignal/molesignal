// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What is kept about a recorded session besides its events: what people said about moments of it,
//! and the summary a model wrote of it, if anyone asked for one.

use serde::Deserialize;
use serde_json::{Value, json};
use tool_runtime::ToolResult;

use crate::{
    app::{
        iam::IamContext,
        tools::{ToolRuntime, common::parse_args},
    },
    shared::Result,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Args {
    session_id: String,
}

pub(super) async fn get(
    runtime: &ToolRuntime,
    auth: &IamContext,
    arguments: Value,
) -> Result<ToolResult> {
    let args: Args = parse_args(arguments)?;
    let session = args.session_id.trim();

    let comments = runtime
        .observability
        .rum_comments
        .list(&auth.org_id, session)
        .await?;

    let summary = runtime
        .observability
        .rum_summaries
        .latest(&auth.org_id, session)
        .await?;

    Ok(ToolResult::json(json!({
        "sessionId": session,
        "comments": comments.iter().map(|comment| json!({
            "atTime": comment.at_time,
            "author": comment.author_name,
            "body": comment.body,
        })).collect::<Vec<_>>(),
        "summary": summary.map(|summary| json!({
            "language": summary.language,
            "summary": summary.summary,
            "chapters": summary.chapters,
            "generatedAt": summary.generated_at.0,
        })),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_named() {
        assert!(parse_args::<Args>(json!({})).is_err());
        assert!(parse_args::<Args>(json!({"sessionId": "s-1"})).is_ok());
    }
}
