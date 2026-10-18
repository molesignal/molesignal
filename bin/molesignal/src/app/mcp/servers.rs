// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 新建、修改与导入服务器。
//!
//! 组织级服务器可以设私网、域名与网段名单，可以关掉证书校验；团队与个人的服务器按组织的
//! 成员网络策略连接，这几项不生效，证书一律校验。stdio 服务器可以存，要在 Mole Runner 上跑。

use std::collections::HashSet;

use ipnet::IpNet;
use reqwest::header::HeaderName;
use serde::{Deserialize, Serialize};

use super::{McpService, transport::validate_endpoint_shape};
use crate::{
    agent::{
        mcp::{
            ImportedServer, NamedValueInput, ValueKind, find_reserved_overlap, normalize_values,
            parse_client_config, unique_handle,
        },
        scope::{Scope, Viewer},
        tool_control::{McpServer, McpServerInput, MemberCredentialSpec},
    },
    shared::{Error, Result, ids::Id},
};

const DEFAULT_TIMEOUT_MS: i64 = 10_000;
const MAX_TIMEOUT_MS: i64 = 120_000;
const DEFAULT_MAX_RESPONSE_BYTES: i64 = 1_048_576;
const MAX_RESPONSE_BYTES: i64 = 16 * 1_048_576;
const MAX_COMMAND_CHARS: usize = 4_096;
const MAX_INSTRUCTIONS_CHARS: usize = 2_000;

/// 新建或修改一台服务器时提交的内容。
#[derive(Debug, Clone, Deserialize)]
pub struct ServerDraft {
    pub name: String,
    #[serde(default = "default_transport")]
    pub transport: String,
    #[serde(default)]
    pub endpoint_url: Option<String>,
    #[serde(default)]
    pub command_template: Option<String>,
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    #[serde(default)]
    pub auth_header: Option<String>,
    /// 只写；留空表示不变。
    #[serde(default)]
    pub credential: Option<String>,
    #[serde(default)]
    pub member_credential: Option<MemberCredentialSpec>,
    #[serde(default)]
    pub headers: Vec<NamedValueInput>,
    #[serde(default)]
    pub env: Vec<NamedValueInput>,
    #[serde(default = "default_true")]
    pub private_only: bool,
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    #[serde(default)]
    pub allowed_cidrs: Vec<String>,
    #[serde(default)]
    pub follow_redirects: bool,
    #[serde(default = "default_true")]
    pub tls_verify: bool,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: i64,
    #[serde(default = "default_max_response_bytes")]
    pub max_response_bytes: i64,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_transport() -> String {
    "streamable_http".into()
}

fn default_auth_type() -> String {
    "none".into()
}

const fn default_true() -> bool {
    true
}

const fn default_timeout_ms() -> i64 {
    DEFAULT_TIMEOUT_MS
}

const fn default_max_response_bytes() -> i64 {
    DEFAULT_MAX_RESPONSE_BYTES
}

/// 认证方式要不要存一个共用的凭据。
fn needs_shared_credential(auth_type: &str) -> bool {
    !matches!(auth_type, "none" | "member_secret")
}

/// 认证方式占用的请求头。
fn auth_header_of(
    auth_type: &str,
    auth_header: Option<&str>,
    member: Option<&MemberCredentialSpec>,
) -> Option<String> {
    match auth_type {
        "bearer_token" | "oauth" => Some("authorization".into()),
        "api_key" => Some(auth_header.unwrap_or("x-api-key").into()),
        "internal_service_identity" => {
            Some(auth_header.unwrap_or("x-internal-service-identity").into())
        }
        "member_secret" => member.map(|spec| spec.header.clone()),
        _ => None,
    }
}

fn normalize_member_credential(spec: Option<MemberCredentialSpec>) -> Result<MemberCredentialSpec> {
    let spec =
        spec.ok_or_else(|| Error::invalid("member credentials need the header that carries them"))?;

    let header = normalize_values(
        ValueKind::Header,
        vec![NamedValueInput {
            name: spec.header,
            secret: true,
            value: None,
        }],
    )?
    .remove(0)
    .name;

    let instructions = spec.instructions.trim().to_string();

    if instructions.chars().count() > MAX_INSTRUCTIONS_CHARS {
        return Err(Error::invalid(format!(
            "instructions for members must be at most {MAX_INSTRUCTIONS_CHARS} characters"
        )));
    }

    Ok(MemberCredentialSpec {
        header,
        bearer: spec.bearer,
        instructions,
    })
}

/// 校验并整理一份草稿。返回写入的内容与共用凭据（只写）。
fn normalize(
    draft: ServerDraft,
    scope: Scope,
    id: Id,
    org_id: Id,
    handle: String,
    created_by: Id,
) -> Result<(McpServerInput, Option<String>)> {
    let name = draft.name.trim();

    if name.is_empty() || name.chars().count() > 255 {
        return Err(Error::invalid(
            "MCP Server name must contain 1 to 255 characters",
        ));
    }

    if !matches!(
        draft.transport.as_str(),
        "streamable_http" | "sse" | "stdio" | "unix_socket"
    ) {
        return Err(Error::invalid("unsupported MCP transport"));
    }

    let endpoint_url = draft
        .endpoint_url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let command_template = draft
        .command_template
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    match draft.transport.as_str() {
        "streamable_http" | "sse" => {
            validate_endpoint_shape(
                endpoint_url
                    .as_deref()
                    .ok_or_else(|| Error::invalid("Endpoint URL is required"))?,
            )?;
        }
        "stdio" if command_template.is_none() => {
            return Err(Error::invalid(
                "a stdio MCP server needs the command to run",
            ));
        }
        _ => {}
    }

    if command_template
        .as_deref()
        .is_some_and(|command| command.chars().count() > MAX_COMMAND_CHARS)
    {
        return Err(Error::invalid(format!(
            "the command must be at most {MAX_COMMAND_CHARS} characters"
        )));
    }

    if draft.follow_redirects {
        return Err(Error::invalid(
            "MCP redirects are blocked by the server security policy",
        ));
    }

    if !matches!(
        draft.auth_type.as_str(),
        "none"
            | "bearer_token"
            | "api_key"
            | "oauth"
            | "mtls"
            | "internal_service_identity"
            | "member_secret"
    ) {
        return Err(Error::invalid("unsupported MCP authentication type"));
    }

    if !(1_000..=MAX_TIMEOUT_MS).contains(&draft.timeout_ms) {
        return Err(Error::invalid(
            "MCP timeout_ms must be between 1000 and 120000",
        ));
    }

    if !(1_024..=MAX_RESPONSE_BYTES).contains(&draft.max_response_bytes) {
        return Err(Error::invalid(
            "MCP max_response_bytes must be between 1024 and 16777216",
        ));
    }

    let auth_header = draft
        .auth_header
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());

    if auth_header.is_some()
        && !matches!(
            draft.auth_type.as_str(),
            "api_key" | "internal_service_identity"
        )
    {
        return Err(Error::invalid(
            "a custom authentication header is only valid for API key or internal identity auth",
        ));
    }

    if let Some(header) = auth_header.as_deref() {
        let parsed = HeaderName::from_bytes(header.as_bytes())
            .map_err(|_| Error::invalid("invalid MCP authentication header"))?;

        if !parsed.as_str().starts_with("x-") {
            return Err(Error::invalid(
                "custom MCP authentication headers must use an X- prefix",
            ));
        }
    }

    let member_credential = if draft.auth_type == "member_secret" {
        Some(normalize_member_credential(draft.member_credential)?)
    } else {
        None
    };

    let headers = normalize_values(ValueKind::Header, draft.headers)?;
    let env = normalize_values(ValueKind::Env, draft.env)?;

    if let Some(used) = auth_header_of(
        &draft.auth_type,
        auth_header.as_deref(),
        member_credential.as_ref(),
    ) && let Some(overlap) = find_reserved_overlap(&headers, &[used.as_str()])
    {
        return Err(Error::invalid(format!(
            "the `{overlap}` header already carries the authentication credential"
        )));
    }

    let credential = draft
        .credential
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    // 团队与个人的服务器按成员网络策略连接，管理员的私网与名单设置不生效，证书一律校验。
    let organization = scope == Scope::Org;

    let allowed_domains = if organization {
        draft
            .allowed_domains
            .into_iter()
            .map(|value| value.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    for domain in &allowed_domains {
        if domain.contains('/') || domain.contains(':') || domain.contains(char::is_whitespace) {
            return Err(Error::invalid(format!(
                "invalid allowed MCP domain `{domain}`"
            )));
        }
    }

    let allowed_cidrs = if organization {
        draft.allowed_cidrs
    } else {
        Vec::new()
    };

    for cidr in &allowed_cidrs {
        cidr.parse::<IpNet>()
            .map_err(|_| Error::invalid(format!("invalid MCP CIDR `{cidr}`")))?;
    }

    Ok((
        McpServerInput {
            id,
            org_id,
            private_only: organization && draft.private_only,
            tls_verify: !organization || draft.tls_verify,
            scope,
            handle,
            name: name.to_string(),
            transport: draft.transport,
            endpoint_url,
            command_template,
            auth_type: draft.auth_type,
            auth_header,
            member_credential,
            headers,
            env,
            allowed_domains,
            allowed_cidrs,
            follow_redirects: false,
            timeout_ms: draft.timeout_ms,
            max_response_bytes: draft.max_response_bytes,
            enabled: draft.enabled,
            created_by,
        },
        credential,
    ))
}

/// 导入的结果：建好的，没建的与原因。
#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub created: Vec<ImportCreated>,
    pub skipped: Vec<ImportSkip>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportCreated {
    #[serde(flatten)]
    pub server: McpServer,
    /// 引用了变量、要补填的请求头与环境变量。
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportSkip {
    pub name: String,
    pub reason: String,
}

/// 导入的一台 → 草稿：不设认证方式，凭据放在请求头里；不限私网。
fn draft_of(server: ImportedServer) -> ServerDraft {
    ServerDraft {
        name: server.name,
        transport: server.transport,
        endpoint_url: server.endpoint_url,
        command_template: server.command,
        auth_type: default_auth_type(),
        auth_header: None,
        credential: None,
        member_credential: None,
        headers: server.headers,
        env: server.env,
        private_only: false,
        allowed_domains: Vec::new(),
        allowed_cidrs: Vec::new(),
        follow_redirects: false,
        tls_verify: true,
        timeout_ms: DEFAULT_TIMEOUT_MS,
        max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        enabled: !server.disabled,
    }
}

impl McpService {
    async fn taken_handles(&self, org_id: &Id) -> Result<HashSet<String>> {
        Ok(self
            .repo
            .list_mcp_servers(org_id)
            .await?
            .into_iter()
            .map(|server| server.handle)
            .collect())
    }

    pub async fn create(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        scope: Scope,
        draft: ServerDraft,
    ) -> Result<McpServer> {
        if !viewer.can_edit(&scope) {
            return Err(Error::forbidden("you cannot add MCP servers to this scope"));
        }

        let handle = unique_handle(&draft.name, &self.taken_handles(org_id).await?);

        let (input, credential) = normalize(
            draft,
            scope,
            Id::new(),
            org_id.clone(),
            handle,
            viewer.user_id.clone(),
        )?;

        self.check_stdio_input(&input)?;

        if needs_shared_credential(&input.auth_type) && credential.is_none() {
            return Err(Error::invalid(
                "credential is required for the selected authentication type",
            ));
        }

        self.repo
            .create_mcp_server(input, credential.as_deref())
            .await
    }

    /// 修改；可以移到另一个本人能管的作用域。返回修改前后的服务器。
    pub async fn update(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        id: &Id,
        scope: Scope,
        draft: ServerDraft,
    ) -> Result<(McpServer, McpServer)> {
        let existing = self.editable(org_id, viewer, id).await?;

        if !viewer.can_edit(&scope) {
            return Err(Error::forbidden(
                "you cannot move MCP servers to this scope",
            ));
        }

        let (input, credential) = normalize(
            draft,
            scope,
            id.clone(),
            org_id.clone(),
            existing.handle.clone(),
            existing.created_by.clone(),
        )?;

        self.check_stdio_input(&input)?;

        if needs_shared_credential(&input.auth_type)
            && credential.is_none()
            && !existing.credential_set
        {
            return Err(Error::invalid(
                "credential is required for the selected authentication type",
            ));
        }

        let server = self
            .repo
            .update_mcp_server(input, credential.as_deref())
            .await?;

        Ok((existing, server))
    }

    /// 粘贴的客户端配置 → 一台或多台服务器；同一作用域里已有同名的跳过。
    pub async fn import(
        &self,
        org_id: &Id,
        viewer: &Viewer,
        scope: Scope,
        config: &serde_json::Value,
    ) -> Result<ImportReport> {
        if !viewer.can_edit(&scope) {
            return Err(Error::forbidden("you cannot add MCP servers to this scope"));
        }

        let imported = parse_client_config(config)?;

        let mut names: HashSet<String> = self
            .repo
            .list_mcp_servers(org_id)
            .await?
            .into_iter()
            .filter(|server| server.scope == scope)
            .map(|server| server.name.to_lowercase())
            .collect();

        let mut handles = self.taken_handles(org_id).await?;

        let mut report = ImportReport {
            created: Vec::new(),
            skipped: Vec::new(),
        };

        for server in imported {
            let name = server.name.clone();
            let missing = server.missing.clone();

            if !names.insert(name.to_lowercase()) {
                report.skipped.push(ImportSkip {
                    name,
                    reason: "an MCP server with this name already exists in this scope".into(),
                });
                continue;
            }

            let handle = unique_handle(&name, &handles);

            let normalized = normalize(
                draft_of(server),
                scope.clone(),
                Id::new(),
                org_id.clone(),
                handle.clone(),
                viewer.user_id.clone(),
            );

            let created = match normalized {
                Ok((input, _)) => match self.check_stdio_input(&input) {
                    Ok(()) => self.repo.create_mcp_server(input, None).await,
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            };

            match created {
                Ok(created) => {
                    handles.insert(handle);
                    report.created.push(ImportCreated {
                        server: created,
                        missing,
                    });
                }
                Err(error) => report.skipped.push(ImportSkip {
                    name,
                    reason: super::safe_runtime_error(&error),
                }),
            }
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn draft(value: serde_json::Value) -> ServerDraft {
        serde_json::from_value(value).unwrap()
    }

    fn normalized(
        scope: Scope,
        value: serde_json::Value,
    ) -> Result<(McpServerInput, Option<String>)> {
        normalize(
            draft(value),
            scope,
            Id::from_string("server"),
            Id::from_string("org"),
            "github".into(),
            Id::from_string("user"),
        )
    }

    #[test]
    fn member_servers_cannot_widen_the_network_or_skip_certificates() {
        let personal = Scope::Personal {
            owner_id: Id::from_string("user"),
        };

        let (input, _) = normalized(
            personal,
            json!({
                "name": "GitHub",
                "endpoint_url": "https://api.githubcopilot.com/mcp/",
                "private_only": true,
                "tls_verify": false,
                "allowed_cidrs": ["10.0.0.0/8"]
            }),
        )
        .unwrap();

        assert!(!input.private_only);
        assert!(input.tls_verify);
        assert!(input.allowed_cidrs.is_empty());

        let (organization, _) = normalized(
            Scope::Org,
            json!({
                "name": "Internal",
                "endpoint_url": "https://mcp.internal/mcp",
                "tls_verify": false,
                "allowed_cidrs": ["10.0.0.0/8"]
            }),
        )
        .unwrap();

        assert!(organization.private_only);
        assert!(!organization.tls_verify);
    }

    #[test]
    fn member_credentials_and_headers_are_checked() {
        let (input, _) = normalized(
            Scope::Org,
            json!({
                "name": "Linear",
                "endpoint_url": "https://mcp.linear.app/mcp",
                "auth_type": "member_secret",
                "member_credential": {"header": "Authorization", "bearer": true, "instructions": " Settings → API "},
                "headers": [{"name": "X-Team", "value": "platform"}]
            }),
        )
        .unwrap();

        let spec = input.member_credential.unwrap();
        assert_eq!(spec.header, "authorization");
        assert_eq!(spec.instructions, "Settings → API");
        assert_eq!(input.headers[0].name, "x-team");

        for value in [
            json!({"name": "x", "endpoint_url": "https://x", "auth_type": "member_secret"}),
            json!({
                "name": "x", "endpoint_url": "https://x", "auth_type": "bearer_token",
                "headers": [{"name": "Authorization", "value": "Bearer y"}]
            }),
            json!({"name": "x", "transport": "sse"}),
            json!({"name": "x", "transport": "stdio"}),
        ] {
            assert!(normalized(Scope::Org, value.clone()).is_err(), "{value}");
        }
    }
}
