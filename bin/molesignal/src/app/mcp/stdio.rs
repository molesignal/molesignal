// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 在 MoleSignal 服务端起一个 stdio MCP 服务器。
//!
//! 只起实例配置 `[agent] mcp_stdio_commands` 列出的程序（原样比较），只给组织级服务器用：
//! 个人与团队的服务器不能让服务端执行命令。不经过 shell，命令行按引号拆成程序与参数；子进程
//! 不继承服务端的环境变量，只拿到 [`INHERITED_ENV`] 与这台服务器配置的变量；工作目录是系统
//! 临时目录，stderr 丢弃（可能带着凭据）。一次会话一个进程，会话结束（这个值被丢掉）时结束
//! 进程。消息每行一个 JSON-RPC，服务器发来的 `ping` 当场回答。

use std::{process::Stdio, time::Duration};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

use super::{
    McpService,
    transport::{initialize_params, max_bytes_of, rpc_result, timeout_of},
};
use crate::{
    agent::{
        scope::Scope,
        tool_control::{McpServerInput, McpServerRuntime},
    },
    shared::{Error, Result},
};

/// 子进程从服务端带过去的环境变量：找得到程序，有家目录与临时目录，按 UTF-8 读写。
const INHERITED_ENV: [&str; 4] = ["PATH", "HOME", "LANG", "TMPDIR"];

/// 命令行 → 程序与参数。程序要在实例允许的名单里；名单是空的就什么都不起。
pub(super) fn allowed_command(command: &str, allowed: &[String]) -> Result<Vec<String>> {
    if allowed.is_empty() {
        return Err(Error::forbidden(
            "this MoleSignal instance runs no stdio MCP servers; an operator allows commands in [agent] mcp_stdio_commands",
        ));
    }

    let words = shlex::split(command)
        .filter(|words| !words.is_empty())
        .ok_or_else(|| Error::invalid("the stdio command cannot be read; check its quotes"))?;

    if !allowed.iter().any(|program| *program == words[0]) {
        return Err(Error::forbidden(format!(
            "`{}` is not a command this instance allows for stdio MCP servers",
            words[0]
        )));
    }

    Ok(words)
}

/// 一台 stdio 服务器能不能存、能不能起（组织级，命令在名单里），能的话它的程序与参数。
pub(super) fn check_stdio(
    scope: &Scope,
    command: Option<&str>,
    allowed: &[String],
) -> Result<Vec<String>> {
    if *scope != Scope::Org {
        return Err(Error::forbidden(
            "only the organization's MCP servers run over stdio; a personal or team server connects to a URL",
        ));
    }

    let command =
        command.ok_or_else(|| Error::invalid("a stdio MCP server needs the command to run"))?;

    allowed_command(command, allowed)
}

/// stdio 服务器不收发 HTTP 请求：凭据只能放进环境变量，认证方式、请求头和成员各自的凭据都用不上，
/// 存下来只会让成员去填一个永远不会被发出去的值。
fn credentials_in_env(input: &McpServerInput) -> Result<()> {
    if input.auth_type != "none" || input.member_credential.is_some() || !input.headers.is_empty() {
        return Err(Error::invalid(
            "a stdio MCP server takes its credentials as environment variables, not as authentication or request headers",
        ));
    }

    Ok(())
}

impl McpService {
    /// 存一台服务器之前：是 stdio 的，要能在这个实例上起，凭据在环境变量里。
    pub(super) fn check_stdio_input(&self, input: &McpServerInput) -> Result<()> {
        if input.transport != "stdio" {
            return Ok(());
        }

        credentials_in_env(input)?;

        check_stdio(
            &input.scope,
            input.command_template.as_deref(),
            &self.stdio_commands,
        )
        .map(|_| ())
    }

    /// 实例允许 stdio 服务器起的程序。
    pub fn stdio_commands(&self) -> &[String] {
        &self.stdio_commands
    }
}

pub(super) struct StdioSession {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    timeout: Duration,
    max_bytes: usize,
}

fn closed(error: std::io::Error) -> Error {
    Error::unavailable(format!("the stdio MCP server stopped: {error}"))
}

impl StdioSession {
    /// 起进程并完成 `initialize`，返回会话与服务器的 `initialize` 结果。
    pub(super) async fn open(
        runtime: &McpServerRuntime,
        allowed: &[String],
    ) -> Result<(Self, Value)> {
        let server = &runtime.server;

        let words = check_stdio(&server.scope, server.command_template.as_deref(), allowed)?;
        let mut process = Command::new(&words[0]);

        process
            .args(&words[1..])
            .env_clear()
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);

        for name in INHERITED_ENV {
            if let Ok(value) = std::env::var(name) {
                process.env(name, value);
            }
        }

        for (name, value) in &runtime.env {
            process.env(name, value);
        }

        let mut child = process.spawn().map_err(|error| {
            Error::unavailable(format!("the stdio MCP server could not start: {error}"))
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::internal("the stdio MCP server has no input"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::internal("the stdio MCP server has no output"))?;

        let mut session = Self {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
            timeout: timeout_of(server),
            max_bytes: max_bytes_of(server),
        };

        let initialized = session.request("initialize", initialize_params()).await?;
        session
            .write(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await?;

        Ok((session, initialized))
    }

    pub(super) async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;

        self.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;

        let response = tokio::time::timeout(self.timeout, self.response(id))
            .await
            .map_err(|_| {
                Error::unavailable(format!(
                    "the stdio MCP server did not answer `{method}` in time"
                ))
            })??;

        rpc_result(method, response)
    }

    async fn write(&mut self, message: &Value) -> Result<()> {
        let mut line = serde_json::to_vec(message)
            .map_err(|error| Error::internal(format!("MCP message: {error}")))?;

        line.push(b'\n');
        self.stdin.write_all(&line).await.map_err(closed)?;
        self.stdin.flush().await.map_err(closed)
    }

    /// 读到这个请求的应答为止：通知跳过，服务器发来的请求当场回答（`ping` 回空结果，别的说
    /// 不支持）。
    async fn response(&mut self, id: u64) -> Result<Value> {
        loop {
            let message = self.read_message().await?;

            if let Some(method) = message.get("method").and_then(Value::as_str) {
                if let Some(request_id) = message.get("id").cloned() {
                    let reply = if method == "ping" {
                        json!({"jsonrpc": "2.0", "id": request_id, "result": {}})
                    } else {
                        json!({
                            "jsonrpc": "2.0",
                            "id": request_id,
                            "error": {"code": -32601, "message": "Mole Agent does not answer this request"}
                        })
                    };

                    self.write(&reply).await?;
                }

                continue;
            }

            if message.get("id").and_then(Value::as_u64) == Some(id) {
                return Ok(message);
            }
        }
    }

    /// 一行一个消息，空行跳过；一行超过应答上限算错。
    async fn read_message(&mut self) -> Result<Value> {
        let mut line = Vec::new();

        loop {
            line.clear();

            let limit = u64::try_from(self.max_bytes)
                .unwrap_or(u64::MAX)
                .saturating_add(1);
            let read = (&mut self.stdout)
                .take(limit)
                .read_until(b'\n', &mut line)
                .await
                .map_err(closed)?;

            if read == 0 {
                return Err(Error::unavailable("the stdio MCP server closed its output"));
            }

            if line.len() > self.max_bytes {
                return Err(Error::payload_too_large(format!(
                    "an MCP message exceeds {} bytes",
                    self.max_bytes
                )));
            }

            let text = String::from_utf8_lossy(&line);

            if text.trim().is_empty() {
                continue;
            }

            return serde_json::from_str(text.trim()).map_err(|_| {
                Error::invalid("the stdio MCP server wrote a line that is not JSON-RPC")
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::{
            mcp::NamedValueInput,
            tool_control::{McpServer, MemberCredentialSpec},
        },
        shared::{ids::Id, time::TimestampMicros},
    };

    /// 一个最小的 stdio MCP 服务器：回答 initialize、tools/list，先 ping 一下再回答 tools/call。
    const FAKE_SERVER: &str = r#"while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*) printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-03-26","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1"}}}\n' "$id" ;;
    *'"method":"tools/list"'*) printf '{"jsonrpc":"2.0","method":"notifications/message","params":{}}\n{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"echo","inputSchema":{"type":"object"}}]}}\n' "$id" ;;
    *'"method":"tools/call"'*) printf '{"jsonrpc":"2.0","id":"p1","method":"ping"}\n{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"%s"}]}}\n' "$id" "${GREETING:-none}" ;;
  esac
done"#;

    fn runtime(scope: Scope, command: &str, env: Vec<(String, String)>) -> McpServerRuntime {
        let now = TimestampMicros::now();

        McpServerRuntime {
            server: McpServer {
                id: Id::from_string("server-1"),
                org_id: Id::from_string("org-1"),
                scope,
                handle: "fake".into(),
                name: "Fake".into(),
                transport: "stdio".into(),
                endpoint_url: None,
                command_template: Some(command.into()),
                auth_type: "none".into(),
                auth_header: None,
                credential_last4: None,
                credential_set: false,
                member_credential: None,
                headers: Vec::new(),
                env: Vec::new(),
                private_only: false,
                allowed_domains: Vec::new(),
                allowed_cidrs: Vec::new(),
                follow_redirects: false,
                tls_verify: true,
                timeout_ms: 5_000,
                max_response_bytes: 65_536,
                enabled: true,
                status: "active".into(),
                last_error: None,
                last_tested_at: None,
                last_synced_at: None,
                created_by: Id::from_string("user-1"),
                created_at: now,
                updated_at: now,
            },
            credential: None,
            headers: Vec::new(),
            env,
            member_credential: None,
        }
    }

    fn fake_command() -> String {
        format!("sh -c '{}'", FAKE_SERVER.replace('\'', r"'\''"))
    }

    #[test]
    fn only_allowed_programs_run() {
        let allowed = vec!["npx".to_string(), "/opt/mcp/server".to_string()];

        assert_eq!(
            allowed_command("npx -y '@scope/server' --flag", &allowed).unwrap(),
            ["npx", "-y", "@scope/server", "--flag"]
        );
        assert!(allowed_command("/opt/mcp/server", &allowed).is_ok());
        assert!(allowed_command("bash -c 'npx x'", &allowed).is_err());
        assert!(allowed_command("./npx", &allowed).is_err());
        assert!(allowed_command("npx 'unclosed", &allowed).is_err());
        assert!(allowed_command("npx", &[]).is_err());
    }

    #[test]
    fn only_the_organizations_servers_run_over_stdio() {
        let allowed = vec!["npx".to_string()];

        assert!(check_stdio(&Scope::Org, Some("npx x"), &allowed).is_ok());
        assert!(check_stdio(&Scope::Org, None, &allowed).is_err());

        let team = Scope::Team {
            team_id: Id::from_string("team-1"),
        };
        let personal = Scope::Personal {
            owner_id: Id::from_string("user-1"),
        };

        assert!(check_stdio(&team, Some("npx x"), &allowed).is_err());
        assert!(check_stdio(&personal, Some("npx x"), &allowed).is_err());
    }

    fn input(auth_type: &str, headers: Vec<NamedValueInput>) -> McpServerInput {
        McpServerInput {
            id: Id::from_string("server-1"),
            org_id: Id::from_string("org-1"),
            scope: Scope::Org,
            handle: "fake".into(),
            name: "Fake".into(),
            transport: "stdio".into(),
            endpoint_url: None,
            command_template: Some("npx x".into()),
            auth_type: auth_type.into(),
            auth_header: None,
            member_credential: None,
            headers,
            env: Vec::new(),
            private_only: false,
            allowed_domains: Vec::new(),
            allowed_cidrs: Vec::new(),
            follow_redirects: false,
            tls_verify: true,
            timeout_ms: 5_000,
            max_response_bytes: 65_536,
            enabled: true,
            created_by: Id::from_string("user-1"),
        }
    }

    #[test]
    fn a_stdio_server_keeps_its_credentials_in_the_environment() {
        assert!(credentials_in_env(&input("none", Vec::new())).is_ok());
        assert!(credentials_in_env(&input("bearer", Vec::new())).is_err());

        let header = NamedValueInput {
            name: "authorization".into(),
            secret: true,
            value: Some("Bearer x".into()),
        };
        assert!(credentials_in_env(&input("none", vec![header])).is_err());

        let member = McpServerInput {
            auth_type: "member_secret".into(),
            member_credential: Some(MemberCredentialSpec {
                header: "authorization".into(),
                bearer: true,
                instructions: String::new(),
            }),
            ..input("none", Vec::new())
        };
        assert!(credentials_in_env(&member).is_err());
    }

    #[tokio::test]
    async fn a_stdio_server_answers_over_its_pipes() {
        let allowed = vec!["sh".to_string()];
        let runtime = runtime(
            Scope::Org,
            &fake_command(),
            vec![("GREETING".into(), "hello".into())],
        );

        let (mut session, initialized) = StdioSession::open(&runtime, &allowed).await.unwrap();
        assert_eq!(initialized["serverInfo"]["name"], "fake");

        let tools = session.request("tools/list", json!({})).await.unwrap();
        assert_eq!(tools["tools"][0]["name"], "echo");

        // The server asks a ping first; the answer still comes, with its own environment.
        let called = session
            .request("tools/call", json!({"name": "echo", "arguments": {}}))
            .await
            .unwrap();
        assert_eq!(called["content"][0]["text"], "hello");
    }

    #[tokio::test]
    async fn a_server_that_is_not_allowed_or_not_the_organizations_does_not_start() {
        let runtime_org = runtime(Scope::Org, &fake_command(), Vec::new());
        assert!(
            StdioSession::open(&runtime_org, &["npx".to_string()])
                .await
                .is_err()
        );

        let runtime_team = runtime(
            Scope::Team {
                team_id: Id::from_string("team-1"),
            },
            &fake_command(),
            Vec::new(),
        );
        assert!(
            StdioSession::open(&runtime_team, &["sh".to_string()])
                .await
                .is_err()
        );
    }
}
