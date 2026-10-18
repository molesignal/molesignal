// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 外部 MCP 服务器：谁能看到、谁能改（规则在 `agent::scope`），连到哪里算安全
//! （`app::egress`），连接、同步与调用。
//!
//! - 组织级服务器由 `agent.manage` 持有人管理，按它自己的私网与名单设置连接；团队与个人的
//!   服务器只能连公网与组织网络策略允许的内网地址，且一律校验 TLS 证书。
//! - 团队会话只用组织与本队的服务器；个人会话用组织的、本人的，以及已经挂进来的团队的。
//! - 成员凭据按人保存；工具调用与 prompt 用发起人自己的，缺了就请他去填。
//! - 同步发现的工具默认关闭；定期重新同步时，schema 变了或消失的工具标为不可用。
//! - stdio 服务器只能是组织级的，由服务端按实例允许的命令起（[`stdio`]）。

mod execution;
mod schema;
mod servers;
mod sse;
mod stdio;
mod sync;
mod transport;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub use execution::AUTHORIZATION_REQUIRED;
pub(crate) use schema::validate_schema;
use serde::Serialize;
pub use servers::{ImportReport, ServerDraft};
pub use sync::{RemoteMcpTool, SyncOutcome, TestOutcome, safe_runtime_error};

use crate::{
    agent::{
        mcp::{McpPrompt, prompt_command},
        network::NetworkPolicyStore,
        scope::{ChatScope, Scope, Viewer},
        tool_control::{McpServer, MemberCredential, ToolControlRepository},
    },
    app::egress::TargetPolicy,
    shared::{Error, Result, ids::Id},
};

pub struct McpService {
    repo: Arc<dyn ToolControlRepository>,
    network: Arc<dyn NetworkPolicyStore>,
    /// 实例允许 stdio 服务器起的程序（`[agent] mcp_stdio_commands`）；空的就不起。
    stdio_commands: Arc<[String]>,
}

/// 列表与详情里的一台服务器，加上看的人能做什么、他自己存的凭据。
#[derive(Debug, Clone, Serialize)]
pub struct ServerView {
    #[serde(flatten)]
    pub server: McpServer,
    pub editable: bool,
    pub moderatable: bool,
    pub tool_count: usize,
    pub prompt_count: usize,
    pub my_credential: Option<MemberCredential>,
}

/// 命令菜单里的一个 prompt。
#[derive(Debug, Clone)]
pub struct SessionPrompt {
    pub server: McpServer,
    pub prompt: McpPrompt,
    /// 不带 `/` 的命令名。
    pub command: String,
    /// 个人会话里调用时要挂进来的团队。
    pub mounts: Option<Id>,
    /// 服务器要成员凭据，而看的人还没存。
    pub needs_credential: bool,
}

impl McpService {
    pub fn new(repo: Arc<dyn ToolControlRepository>, network: Arc<dyn NetworkPolicyStore>) -> Self {
        Self {
            repo,
            network,
            stdio_commands: Arc::from(Vec::new()),
        }
    }

    /// 允许 stdio 服务器在服务端起的程序。
    pub fn with_stdio_commands(mut self, commands: Vec<String>) -> Self {
        self.stdio_commands = Arc::from(commands);
        self
    }

    /// 连接一台服务器时用的地址规则。
    async fn policy_for(&self, server: &McpServer) -> Result<TargetPolicy> {
        if server.scope != Scope::Org {
            return Ok(TargetPolicy::member(
                &self.network.get(&server.org_id).await?,
            ));
        }

        let allowed_cidrs = server
            .allowed_cidrs
            .iter()
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| Error::invalid(format!("invalid stored MCP CIDR `{value}`")))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(TargetPolicy::Admin {
            private_only: server.private_only,
            allowed_domains: server.allowed_domains.clone(),
            allowed_cidrs,
        })
    }

    /// 一台看得到的服务器；看不到的与不存在一样回 404。
    pub async fn visible(&self, org_id: &Id, viewer: &Viewer, id: &Id) -> Result<McpServer> {
        let server = self.repo.get_mcp_server(org_id, id).await?;

        if !viewer.can_see(&server.scope) {
            return Err(Error::not_found(format!("MCP Server `{}` not found", id.0)));
        }

        Ok(server)
    }

    /// 一台能改的服务器。
    pub async fn editable(&self, org_id: &Id, viewer: &Viewer, id: &Id) -> Result<McpServer> {
        let server = self.visible(org_id, viewer, id).await?;

        if !viewer.can_edit(&server.scope) {
            return Err(Error::forbidden(
                "only the people who manage this MCP server's scope can change it",
            ));
        }

        Ok(server)
    }

    /// 一台能停用、删除的服务器。
    pub async fn moderatable(&self, org_id: &Id, viewer: &Viewer, id: &Id) -> Result<McpServer> {
        let server = self.visible(org_id, viewer, id).await?;

        if !viewer.can_moderate(&server.scope) {
            return Err(Error::forbidden(
                "only the people who manage this MCP server's scope can disable or remove it",
            ));
        }

        Ok(server)
    }

    /// 看得到的服务器，最近改过的在前。
    pub async fn list(&self, org_id: &Id, viewer: &Viewer) -> Result<Vec<ServerView>> {
        let servers = self.repo.list_mcp_servers(org_id).await?;
        let tools = self.repo.list_mcp_tools(org_id, None).await?;
        let prompts = self.repo.list_mcp_prompts(org_id).await?;

        let credentials = self
            .repo
            .list_member_credentials(org_id, &viewer.user_id)
            .await?
            .into_iter()
            .map(|credential| (credential.server_id.clone(), credential))
            .collect::<HashMap<_, _>>();

        let count = |ids: Vec<&Id>| {
            ids.into_iter()
                .fold(HashMap::<Id, usize>::new(), |mut counts, id| {
                    *counts.entry(id.clone()).or_default() += 1;
                    counts
                })
        };

        let tool_counts = count(tools.iter().map(|tool| &tool.server_id).collect());
        let prompt_counts = count(prompts.iter().map(|prompt| &prompt.server_id).collect());

        Ok(servers
            .into_iter()
            .filter(|server| viewer.can_see(&server.scope))
            .map(|server| ServerView {
                editable: viewer.can_edit(&server.scope),
                moderatable: viewer.can_moderate(&server.scope),
                tool_count: tool_counts.get(&server.id).copied().unwrap_or_default(),
                prompt_count: prompt_counts.get(&server.id).copied().unwrap_or_default(),
                my_credential: credentials.get(&server.id).cloned(),
                server,
            })
            .collect())
    }

    pub async fn view(&self, org_id: &Id, viewer: &Viewer, id: &Id) -> Result<ServerView> {
        let server = self.visible(org_id, viewer, id).await?;

        self.list(org_id, viewer)
            .await?
            .into_iter()
            .find(|view| view.server.id == server.id)
            .ok_or_else(|| Error::not_found(format!("MCP Server `{}` not found", id.0)))
    }

    pub async fn prompts_of(&self, org_id: &Id, server_id: &Id) -> Result<Vec<McpPrompt>> {
        Ok(self
            .repo
            .list_mcp_prompts(org_id)
            .await?
            .into_iter()
            .filter(|prompt| prompt.server_id == *server_id)
            .collect())
    }

    pub async fn set_enabled(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        id: &Id,
        enabled: bool,
    ) -> Result<McpServer> {
        self.moderatable(org_id, viewer, id).await?;
        self.repo.set_mcp_server_enabled(org_id, id, enabled).await
    }

    /// 本人给一台要成员凭据的服务器存自己的值。
    pub async fn put_my_credential(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        id: &Id,
        value: &str,
    ) -> Result<(McpServer, MemberCredential)> {
        let server = self.visible(org_id, viewer, id).await?;

        if server.auth_type != "member_secret" {
            return Err(Error::invalid(
                "this MCP server does not take credentials from its members",
            ));
        }

        let value = value.trim();

        if value.is_empty() || value.len() > 8 * 1024 {
            return Err(Error::invalid("a credential must contain 1 to 8192 bytes"));
        }

        if reqwest::header::HeaderValue::from_str(value).is_err() {
            return Err(Error::invalid(
                "the credential contains characters a header cannot carry",
            ));
        }

        let credential = self
            .repo
            .put_member_credential(org_id, id, &viewer.user_id, value)
            .await?;

        Ok((server, credential))
    }

    pub async fn delete_my_credential(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        id: &Id,
    ) -> Result<(McpServer, bool)> {
        let server = self.visible(org_id, viewer, id).await?;

        let removed = self
            .repo
            .delete_member_credential(org_id, id, &viewer.user_id)
            .await?;

        Ok((server, removed))
    }

    /// 会话里能用的服务器。
    pub async fn session_servers(&self, org_id: &Id, chat: &ChatScope) -> Result<Vec<McpServer>> {
        Ok(self
            .repo
            .list_mcp_servers(org_id)
            .await?
            .into_iter()
            .filter(|server| chat.loads(&server.scope))
            .collect())
    }

    /// 命令菜单里的 prompt：会话里能用的服务器的，加上个人会话里本人所在团队、还没挂进来的。
    pub async fn session_prompts(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        chat: &ChatScope,
    ) -> Result<Vec<SessionPrompt>> {
        let saved: HashSet<Id> = self
            .repo
            .list_member_credentials(org_id, &viewer.user_id)
            .await?
            .into_iter()
            .map(|credential| credential.server_id)
            .collect();

        let mut servers = HashMap::new();

        for server in self.repo.list_mcp_servers(org_id).await? {
            if !server.enabled || server.status != "healthy" {
                continue;
            }

            let mounts = match (&server.scope, chat) {
                _ if chat.loads(&server.scope) => None,
                (Scope::Team { team_id }, ChatScope::Personal { .. })
                    if viewer.is_member(team_id) =>
                {
                    Some(team_id.clone())
                }
                _ => continue,
            };

            servers.insert(server.id.clone(), (server, mounts));
        }

        let mut seen = HashSet::new();
        let mut prompts = Vec::new();

        for prompt in self.repo.list_mcp_prompts(org_id).await? {
            let Some((server, mounts)) = servers.get(&prompt.server_id) else {
                continue;
            };

            let command = prompt_command(&server.handle, &prompt.name);

            if !seen.insert(command.clone()) {
                continue;
            }

            prompts.push(SessionPrompt {
                needs_credential: server.auth_type == "member_secret"
                    && !saved.contains(&server.id),
                server: server.clone(),
                prompt,
                command,
                mounts: mounts.clone(),
            });
        }

        prompts.sort_by(|left, right| left.command.cmp(&right.command));
        Ok(prompts)
    }
}
