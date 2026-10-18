// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `POST /api/v1/agent/mcp-servers/import`：粘贴 Claude Desktop、Cursor、VS Code 等客户端的
//! `mcpServers` 配置，一次建一台或多台服务器。`config` 可以是 JSON 对象，也可以是原样粘贴的
//! 文本。建好的服务器还没同步；引用了变量的请求头与环境变量要补填，stdio 服务器要在 Mole
//! Runner 上运行。

use axum::{Extension, Json, extract::State};
use serde::Deserialize;
use serde_json::{Value, json};

use super::super::viewer::{requested_scope, viewer};
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::iam::IamContext,
    domain::iam::permission,
    shared::{Error, Result},
};

/// 粘贴内容的上限。
const MAX_CONFIG_BYTES: usize = 256 * 1024;

#[derive(Debug, Deserialize)]
pub(super) struct ImportRequest {
    #[serde(default = "personal")]
    scope: String,
    #[serde(default)]
    team_id: Option<String>,
    config: Value,
}

fn personal() -> String {
    "personal".into()
}

/// 粘贴的文本 → JSON。客户端的配置文件常带注释与尾逗号，这里只接受标准 JSON。
fn config_value(config: Value) -> Result<Value> {
    match config {
        Value::String(text) => {
            if text.len() > MAX_CONFIG_BYTES {
                return Err(Error::invalid("the configuration is too large to import"));
            }

            serde_json::from_str(text.trim()).map_err(|error| {
                Error::invalid(format!("the configuration is not valid JSON: {error}"))
            })
        }
        value => Ok(value),
    }
}

#[permission("agent.use")]
pub(super) async fn import_servers(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Json(request): Json<ImportRequest>,
) -> Result<Json<Value>> {
    let scope = requested_scope(&state, &ctx, &request.scope, request.team_id.as_deref()).await?;
    let viewer = viewer(&state, &ctx).await?;
    let config = config_value(request.config)?;

    let report = state
        .agent
        .mcp
        .import(&ctx.org_id, &viewer, scope.clone(), &config)
        .await?;

    for created in &report.created {
        activity_audit::record(
            &state,
            &ctx,
            "agent.mcp_server.imported",
            "agent_mcp_server",
            &created.server.id.0,
            json!({
                "name": created.server.name,
                "scope": scope.kind(),
                "transport": created.server.transport,
                "missing": created.missing,
            }),
        )
        .await;
    }

    Ok(Json(json!(report)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_text_and_objects_are_both_accepted() {
        let text = Value::String(r#" {"mcpServers": {"x": {"url": "https://x"}}} "#.into());

        assert!(config_value(text).unwrap()["mcpServers"].is_object());
        assert!(config_value(json!({"servers": {}})).unwrap()["servers"].is_object());
        assert!(config_value(Value::String("{ not json".into())).is_err());
    }
}
