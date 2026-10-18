// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 测试与同步。
//!
//! 同步列出服务器的工具与 prompt：选中的工具写进注册表（新工具默认关闭，风险下限取
//! annotations 的保守推断），服务器不再列出的标为不可用；prompt 照新的列表换掉。定期重新
//! 同步只核对已经导入的工具：schema 变了、消失了或者被标得更危险的标为不可用，等有权限的人
//! 看过再同步；靠成员凭据的服务器没有可用的身份，不在后台同步。

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{McpService, schema, transport::Session};
use crate::{
    agent::{
        mcp::{McpPrompt, McpPromptArgument, tool_registry_name},
        model::RiskLevel,
        scope::Viewer,
        tool_control::{ManagedToolStatus, McpServer, McpTool, ToolExecutionMode},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 一次最多翻几页 `tools/list`、`prompts/list`。
const MAX_PAGES: usize = 20;
/// 多久核对一次。
const RESYNC_EVERY_MICROS: i64 = 60 * 60 * 1_000_000;
const RESYNC_BATCH: i64 = 50;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RemoteMcpTool {
    pub name: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(rename = "inputSchema", default = "empty_object_schema")]
    pub input_schema: Value,
    #[serde(rename = "outputSchema", default)]
    pub output_schema: Option<Value>,
    #[serde(default)]
    pub annotations: Value,
}

fn empty_object_schema() -> Value {
    json!({"type": "object", "properties": {}})
}

#[derive(Debug, Clone, Deserialize)]
struct RemotePrompt {
    name: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    arguments: Vec<McpPromptArgument>,
}

/// 连上服务器看到的东西；服务器没声明 prompt 能力时 `prompts` 为空。
pub struct Discovery {
    pub tools: Vec<RemoteMcpTool>,
    pub prompts: Option<Vec<McpPrompt>>,
}

/// 测试的结果：失败也是结果，原因写进服务器状态。
pub struct TestOutcome {
    pub server: McpServer,
    pub tools: Vec<RemoteMcpTool>,
    pub error: Option<String>,
}

pub struct SyncOutcome {
    pub server: McpServer,
    pub tools: Vec<McpTool>,
    pub prompts: usize,
}

async fn list_all(session: &mut Session, method: &str, key: &str) -> Result<Vec<Value>> {
    let mut cursor: Option<String> = None;
    let mut items = Vec::new();

    for _ in 0..MAX_PAGES {
        let params = match &cursor {
            Some(cursor) => json!({"cursor": cursor}),
            None => json!({}),
        };

        let result = session.call(method, params).await?;
        items.extend(result[key].as_array().cloned().unwrap_or_default());

        match result["nextCursor"].as_str() {
            Some(next) if !next.is_empty() => cursor = Some(next.to_string()),
            _ => break,
        }
    }

    Ok(items)
}

pub(super) fn inferred_minimum_risk(annotations: &Value) -> RiskLevel {
    if annotations["destructiveHint"].as_bool().unwrap_or(false) {
        RiskLevel::L4
    } else if annotations["readOnlyHint"].as_bool().unwrap_or(false) {
        RiskLevel::L0
    } else {
        RiskLevel::L2
    }
}

/// 能给用户看的错误原因；别的说成连接失败。
pub fn safe_runtime_error(error: &Error) -> String {
    match error {
        Error::InvalidArgument(message)
        | Error::Forbidden(message)
        | Error::Unauthorized(message)
        | Error::PayloadTooLarge(message)
        | Error::Unavailable(message) => message.chars().take(500).collect(),
        _ => "MCP Server connection failed".into(),
    }
}

fn unavailable_schema(
    schema_value: &Value,
    error: &Error,
) -> (Value, String, String, Option<String>) {
    let diagnostic = error.to_string().chars().take(500).collect::<String>();
    let raw = crate::shared::contracts::canonical_json(schema_value);
    let hash =
        crate::shared::contracts::sha256_hex(crate::shared::contracts::canonical_json_bytes(&raw));

    let dialect = schema_value
        .get("$schema")
        .and_then(Value::as_str)
        .unwrap_or(schema::DIALECT_2020_12)
        .chars()
        .take(128)
        .collect();

    (raw, hash, dialect, Some(diagnostic))
}

/// 一个服务器工具 → 注册表里的一行；已经导入过的保留名称、启用状态与收紧过的风险。
fn tool_row(
    server: &McpServer,
    remote: RemoteMcpTool,
    old: Option<&McpTool>,
    name: String,
    now: TimestampMicros,
) -> McpTool {
    let minimum_risk = inferred_minimum_risk(&remote.annotations);

    let risk = old
        .map(|tool| tool.risk.max(minimum_risk))
        .unwrap_or(minimum_risk);

    let execution_mode = old
        .map(|tool| tool.execution_mode)
        .filter(|mode| mode.allowed_for_risk(risk))
        .unwrap_or_else(|| ToolExecutionMode::default_for_risk(risk));

    let enabled = old.is_some_and(|tool| tool.enabled);
    let remote_input_schema = remote.input_schema;

    let (input_schema, schema_hash, schema_dialect, unavailable_diagnostic) =
        match schema::synchronize(remote_input_schema.clone()) {
            Ok(schema) => (schema.schema, schema.hash, schema.dialect, None),
            Err(error) => unavailable_schema(&remote_input_schema, &error),
        };

    let status = if unavailable_diagnostic.is_some() {
        ManagedToolStatus::Unavailable
    } else if enabled {
        ManagedToolStatus::Healthy
    } else {
        ManagedToolStatus::Disabled
    };

    McpTool {
        id: old.map(|tool| tool.id.clone()).unwrap_or_default(),
        org_id: server.org_id.clone(),
        server_id: server.id.clone(),
        remote_name: remote.name.clone(),
        name,
        display_name: remote.title.clone().unwrap_or_else(|| remote.name.clone()),
        description: remote.description,
        input_schema,
        schema_hash,
        schema_dialect,
        schema_synced_at: now,
        unavailable_diagnostic,
        output_schema: remote.output_schema,
        minimum_risk,
        risk,
        execution_mode,
        capabilities: json!({
            "read_only": remote.annotations["readOnlyHint"].as_bool().unwrap_or(false),
            "supports_dry_run": false,
            "idempotent": remote.annotations["idempotentHint"].as_bool().unwrap_or(false),
            "streaming": false
        }),
        tags: vec!["MCP".into()],
        enabled,
        status,
        version: None,
        last_synced_at: now,
        created_at: old.map(|tool| tool.created_at).unwrap_or(now),
        updated_at: now,
    }
}

/// 核对一个已经导入的工具；有变化时返回不可用的原因。
fn drift(tool: &McpTool, remote: Option<&RemoteMcpTool>) -> Option<&'static str> {
    let Some(remote) = remote else {
        return Some("The MCP server no longer lists this tool; synchronize the server again.");
    };

    let hash = schema::synchronize(remote.input_schema.clone())
        .map(|schema| schema.hash)
        .ok();

    if hash.as_deref() != Some(tool.schema_hash.as_str()) {
        Some("The input schema changed on the MCP server; review it and synchronize again.")
    } else if inferred_minimum_risk(&remote.annotations) > tool.minimum_risk {
        Some("The MCP server now marks this tool as riskier; review it and synchronize again.")
    } else {
        None
    }
}

impl McpService {
    /// 连上服务器，列出工具与 prompt。`member` 是这次操作的人，靠成员凭据的服务器用他的凭据。
    pub(super) async fn discover(
        &self,
        server: &McpServer,
        member: Option<&Id>,
    ) -> Result<Discovery> {
        let runtime = self
            .repo
            .get_mcp_server_runtime(&server.org_id, &server.id, member)
            .await?;

        let policy = self.policy_for(&runtime.server).await?;
        let (mut session, initialized) =
            Session::open(&runtime, &policy, &self.stdio_commands).await?;

        let tools = serde_json::from_value(Value::Array(
            list_all(&mut session, "tools/list", "tools").await?,
        ))
        .map_err(|error| Error::invalid(format!("invalid MCP tools/list response: {error}")))?;

        let prompts = if initialized["capabilities"].get("prompts").is_some() {
            let listed: Vec<RemotePrompt> = serde_json::from_value(Value::Array(
                list_all(&mut session, "prompts/list", "prompts").await?,
            ))
            .map_err(|error| {
                Error::invalid(format!("invalid MCP prompts/list response: {error}"))
            })?;

            let now = TimestampMicros::now();

            Some(
                listed
                    .into_iter()
                    .map(|prompt| McpPrompt {
                        org_id: server.org_id.clone(),
                        server_id: server.id.clone(),
                        name: prompt.name,
                        title: prompt.title,
                        description: prompt.description.unwrap_or_default(),
                        arguments: prompt.arguments,
                        synced_at: now,
                    })
                    .collect(),
            )
        } else {
            None
        };

        Ok(Discovery { tools, prompts })
    }

    pub async fn test(&self, org_id: &Id, viewer: &Viewer, id: &Id) -> Result<TestOutcome> {
        let server = self.editable(org_id, viewer, id).await?;
        let now = TimestampMicros::now();

        match self.discover(&server, Some(&viewer.user_id)).await {
            Ok(discovery) => Ok(TestOutcome {
                server: self
                    .repo
                    .update_mcp_server_runtime_status(org_id, id, "healthy", None, Some(now), None)
                    .await?,
                tools: discovery.tools,
                error: None,
            }),
            Err(error) => {
                let message = safe_runtime_error(&error);

                Ok(TestOutcome {
                    server: self
                        .repo
                        .update_mcp_server_runtime_status(
                            org_id,
                            id,
                            "error",
                            Some(&message),
                            Some(now),
                            None,
                        )
                        .await?,
                    tools: Vec::new(),
                    error: Some(message),
                })
            }
        }
    }

    /// 同步：`selected` 为空时导入全部工具。
    pub async fn sync(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        id: &Id,
        selected: &[String],
    ) -> Result<SyncOutcome> {
        let server = self.editable(org_id, viewer, id).await?;

        let discovery = self
            .discover(&server, Some(&viewer.user_id))
            .await
            .map_err(|error| Error::unavailable(safe_runtime_error(&error)))?;

        let selection: HashSet<&str> = selected.iter().map(String::as_str).collect();
        let listed: HashSet<String> = discovery
            .tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect();

        let organization_tools = self.repo.list_mcp_tools(org_id, None).await?;

        let existing = organization_tools
            .iter()
            .filter(|tool| tool.server_id == *id)
            .map(|tool| (tool.remote_name.clone(), tool.clone()))
            .collect::<HashMap<_, _>>();

        let mut taken: HashSet<String> = organization_tools
            .iter()
            .filter(|tool| tool.server_id != *id)
            .map(|tool| tool.name.clone())
            .chain(existing.values().map(|tool| tool.name.clone()))
            .collect();

        let now = TimestampMicros::now();
        let mut rows = Vec::new();

        for remote in discovery.tools {
            if !selection.is_empty() && !selection.contains(remote.name.as_str()) {
                continue;
            }

            let old = existing.get(&remote.name);

            let name = match old {
                Some(tool) => tool.name.clone(),
                None => {
                    let name = tool_registry_name(&server.handle, &remote.name, &taken);
                    taken.insert(name.clone());
                    name
                }
            };

            rows.push(tool_row(&server, remote, old, name, now));
        }

        for tool in existing
            .values()
            .filter(|tool| !listed.contains(&tool.remote_name))
        {
            rows.push(McpTool {
                status: ManagedToolStatus::Unavailable,
                unavailable_diagnostic: drift(tool, None).map(str::to_string),
                updated_at: now,
                ..tool.clone()
            });
        }

        self.repo.upsert_mcp_tools(rows).await?;

        let prompts = match discovery.prompts {
            Some(prompts) => {
                let count = prompts.len();
                self.repo.replace_mcp_prompts(org_id, id, prompts).await?;
                count
            }
            None => {
                self.repo
                    .replace_mcp_prompts(org_id, id, Vec::new())
                    .await?;
                0
            }
        };

        let server = self
            .repo
            .update_mcp_server_runtime_status(org_id, id, "healthy", None, Some(now), Some(now))
            .await?;

        Ok(SyncOutcome {
            server,
            tools: self.repo.list_mcp_tools(org_id, Some(id)).await?,
            prompts,
        })
    }

    async fn resync(&self, server: &McpServer) -> Result<usize> {
        let now = TimestampMicros::now();

        let discovery = match self.discover(server, None).await {
            Ok(discovery) => discovery,
            Err(error) => {
                let message = safe_runtime_error(&error);

                self.repo
                    .update_mcp_server_runtime_status(
                        &server.org_id,
                        &server.id,
                        "error",
                        Some(&message),
                        Some(now),
                        None,
                    )
                    .await?;

                return Err(error);
            }
        };

        let remote: HashMap<&str, &RemoteMcpTool> = discovery
            .tools
            .iter()
            .map(|tool| (tool.name.as_str(), tool))
            .collect();

        let changed = self
            .repo
            .list_mcp_tools(&server.org_id, Some(&server.id))
            .await?
            .into_iter()
            .filter(|tool| tool.status != ManagedToolStatus::Unavailable)
            .filter_map(|tool| {
                drift(&tool, remote.get(tool.remote_name.as_str()).copied()).map(|reason| McpTool {
                    status: ManagedToolStatus::Unavailable,
                    unavailable_diagnostic: Some(reason.to_string()),
                    updated_at: now,
                    ..tool
                })
            })
            .collect::<Vec<_>>();

        let count = changed.len();
        self.repo.upsert_mcp_tools(changed).await?;

        if let Some(prompts) = discovery.prompts {
            self.repo
                .replace_mcp_prompts(&server.org_id, &server.id, prompts)
                .await?;
        }

        self.repo
            .update_mcp_server_runtime_status(
                &server.org_id,
                &server.id,
                "healthy",
                None,
                Some(now),
                Some(now),
            )
            .await?;

        Ok(count)
    }

    /// 后台：核对到了时间的服务器，返回核对了几台。
    pub async fn resync_due(&self) -> usize {
        let before = TimestampMicros(TimestampMicros::now().0 - RESYNC_EVERY_MICROS);

        let servers = match self.repo.mcp_servers_due(before, RESYNC_BATCH).await {
            Ok(servers) => servers,
            Err(error) => {
                tracing::warn!(%error, "MCP servers due for a check could not be listed");
                return 0;
            }
        };

        for server in &servers {
            match self.resync(server).await {
                Ok(0) => {}
                Ok(changed) => tracing::info!(
                    server = %server.id.0,
                    changed,
                    "MCP tools changed on the server and were marked unavailable"
                ),
                Err(error) => tracing::warn!(
                    server = %server.id.0,
                    error = %safe_runtime_error(&error),
                    "MCP server check failed"
                ),
            }
        }

        servers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(schema: Value, annotations: Value) -> RemoteMcpTool {
        RemoteMcpTool {
            name: "search".into(),
            title: None,
            description: String::new(),
            input_schema: schema,
            output_schema: None,
            annotations,
        }
    }

    #[test]
    fn destructive_remote_tools_have_an_l4_floor() {
        assert_eq!(
            inferred_minimum_risk(&json!({"destructiveHint": true})),
            RiskLevel::L4
        );
        assert_eq!(
            inferred_minimum_risk(&json!({"readOnlyHint": true})),
            RiskLevel::L0
        );
    }

    #[test]
    fn changed_missing_or_riskier_tools_are_noticed() {
        let schema = json!({"type": "object", "properties": {"q": {"type": "string"}}});
        let synchronized = schema::synchronize(schema.clone()).unwrap();

        let tool = McpTool {
            id: Id::from_string("tool"),
            org_id: Id::from_string("org"),
            server_id: Id::from_string("server"),
            remote_name: "search".into(),
            name: "mcp__github__search".into(),
            display_name: "search".into(),
            description: String::new(),
            input_schema: synchronized.schema.clone(),
            schema_hash: synchronized.hash.clone(),
            schema_dialect: synchronized.dialect.clone(),
            schema_synced_at: TimestampMicros(1),
            unavailable_diagnostic: None,
            output_schema: None,
            minimum_risk: RiskLevel::L0,
            risk: RiskLevel::L0,
            execution_mode: ToolExecutionMode::Automatic,
            capabilities: json!({"read_only": true}),
            tags: vec!["MCP".into()],
            enabled: true,
            status: ManagedToolStatus::Healthy,
            version: None,
            last_synced_at: TimestampMicros(1),
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        };

        let same = remote(schema.clone(), json!({"readOnlyHint": true}));
        assert_eq!(drift(&tool, Some(&same)), None);

        let changed = remote(
            json!({"type": "object", "properties": {"q": {"type": "integer"}}}),
            json!({"readOnlyHint": true}),
        );
        assert!(
            drift(&tool, Some(&changed))
                .unwrap()
                .contains("schema changed")
        );

        let riskier = remote(schema, json!({"destructiveHint": true}));
        assert!(drift(&tool, Some(&riskier)).unwrap().contains("riskier"));

        assert!(drift(&tool, None).unwrap().contains("no longer lists"));
    }
}
