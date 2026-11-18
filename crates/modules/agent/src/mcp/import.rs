// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Paste another MCP client's configuration to import one or more servers at once.
//!
//! Recognizes Claude Desktop / Claude Code / Cursor's `{"mcpServers": {...}}`, VS Code's
//! `{"servers": {...}}` and `{"mcp": {"servers": {...}}}` in settings, or a direct
//! `{name: config}`. Servers with `url` are ready to use (connect using legacy SSE when `type` is
//! `sse` or the address ends in `/sse`); servers with `command` are saved to run on Mole Runner.
//! Values referencing variables such as `${input:…}` or `${env:…}` are not imported; they are
//! recorded in `missing` to be filled in after import.

use kernel::{Error, Result};
use serde_json::{Map, Value};

use super::values::NamedValueInput;

const MAX_SERVERS: usize = 50;

/// A parsed server that has not yet been validated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedServer {
    pub name: String,
    /// `streamable_http`, `sse`, or `stdio`.
    pub transport: String,
    pub endpoint_url: Option<String>,
    /// The command line for a stdio server (command plus arguments).
    pub command: Option<String>,
    pub headers: Vec<NamedValueInput>,
    pub env: Vec<NamedValueInput>,
    /// Headers and environment variables that reference variables and need filling in after import.
    pub missing: Vec<String>,
    /// Marked as `disabled` in the original configuration.
    pub disabled: bool,
}

/// Locations where different clients store their server lists.
const SERVER_PATHS: &[&[&str]] = &[&["mcpServers"], &["servers"], &["mcp", "servers"]];

fn servers_map(value: &Value) -> Option<&Map<String, Value>> {
    for path in SERVER_PATHS {
        let found = path
            .iter()
            .try_fold(value, |value, key| value.get(*key))
            .and_then(Value::as_object);

        if found.is_some() {
            return found;
        }
    }

    let map = value.as_object()?;

    let all_servers = !map.is_empty()
        && map.values().all(|server| {
            server.get("url").is_some()
                || server.get("serverUrl").is_some()
                || server.get("httpUrl").is_some()
                || server.get("command").is_some()
        });

    all_servers.then_some(map)
}

fn uses_variable(value: &str) -> bool {
    value.contains("${")
}

/// Determines whether to quote an argument: wraps it in single quotes if it contains whitespace
/// or quotes.
fn quote(argument: &str) -> String {
    if !argument.is_empty()
        && !argument
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '\'' | '"' | '\\' | '$' | '`'))
    {
        return argument.to_string();
    }

    format!("'{}'", argument.replace('\'', r"'\''"))
}

/// Converts a header or environment variable object to a list; skips values that reference
/// variables and marks them as sensitive values to be filled in.
fn named_values(value: Option<&Value>, missing: &mut Vec<String>) -> Result<Vec<NamedValueInput>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };

    let map = value
        .as_object()
        .ok_or_else(|| Error::invalid("headers and env must be objects of names to values"))?;

    map.iter()
        .map(|(name, value)| {
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Number(number) => number.to_string(),
                Value::Bool(flag) => flag.to_string(),
                _ => {
                    return Err(Error::invalid(format!(
                        "the value of `{name}` must be text"
                    )));
                }
            };

            if uses_variable(&text) {
                missing.push(name.clone());

                return Ok(NamedValueInput {
                    name: name.clone(),
                    secret: true,
                    value: None,
                });
            }

            Ok(NamedValueInput {
                name: name.clone(),
                secret: false,
                value: Some(text),
            })
        })
        .collect()
}

fn transport_of(config: &Value, url: Option<&str>) -> Result<String> {
    let declared = config
        .get("type")
        .or_else(|| config.get("transport"))
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);

    let transport = match declared.as_deref() {
        Some("http" | "streamable-http" | "streamable_http" | "streamablehttp") => {
            "streamable_http"
        }
        Some("sse") => "sse",
        Some("stdio") => "stdio",
        Some(other) => {
            return Err(Error::invalid(format!("unknown MCP transport `{other}`")));
        }
        None if config.get("command").is_some() => "stdio",
        None if config.get("httpUrl").is_some() => "streamable_http",
        None if url.is_some_and(|url| url.trim_end_matches('/').ends_with("/sse")) => "sse",
        None => "streamable_http",
    };

    Ok(transport.to_string())
}

fn imported_server(name: &str, config: &Value) -> Result<ImportedServer> {
    if !config.is_object() {
        return Err(Error::invalid(format!(
            "the configuration of `{name}` must be an object"
        )));
    }

    let url = ["url", "serverUrl", "httpUrl"]
        .iter()
        .find_map(|key| config.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|url| !url.is_empty());

    let transport = transport_of(config, url)?;
    let mut missing = Vec::new();
    let headers = named_values(config.get("headers"), &mut missing)?;
    let env = named_values(config.get("env"), &mut missing)?;

    let command = if transport == "stdio" {
        let program = config
            .get("command")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .ok_or_else(|| Error::invalid(format!("`{name}` has no command to run")))?;

        let arguments = match config.get("args") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(values)) => values
                .iter()
                .map(|value| match value {
                    Value::String(text) => Ok(text.clone()),
                    Value::Number(number) => Ok(number.to_string()),
                    _ => Err(Error::invalid(format!("the args of `{name}` must be text"))),
                })
                .collect::<Result<Vec<_>>>()?,
            Some(_) => {
                return Err(Error::invalid(format!(
                    "the args of `{name}` must be a list"
                )));
            }
        };

        Some(
            std::iter::once(quote(program))
                .chain(arguments.iter().map(|argument| quote(argument)))
                .collect::<Vec<_>>()
                .join(" "),
        )
    } else {
        None
    };

    if transport != "stdio" && url.is_none() {
        return Err(Error::invalid(format!("`{name}` has no url")));
    }

    Ok(ImportedServer {
        name: name.trim().to_string(),
        endpoint_url: (transport != "stdio").then(|| url.unwrap_or_default().to_string()),
        transport,
        command,
        headers,
        env,
        missing,
        disabled: config
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// Parses a client configuration. Identifies the problematic server if the format is invalid.
pub fn parse_client_config(value: &Value) -> Result<Vec<ImportedServer>> {
    let servers = servers_map(value).ok_or_else(|| {
        Error::invalid("no MCP servers found; paste an object with mcpServers or servers")
    })?;

    if servers.is_empty() {
        return Err(Error::invalid("the configuration lists no MCP servers"));
    }

    if servers.len() > MAX_SERVERS {
        return Err(Error::invalid(format!(
            "at most {MAX_SERVERS} servers can be imported at once"
        )));
    }

    servers
        .iter()
        .map(|(name, config)| imported_server(name, config))
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn claude_desktop_and_cursor_configurations_are_read() {
        let servers = parse_client_config(&json!({
            "mcpServers": {
                "github": {
                    "url": "https://api.githubcopilot.com/mcp/",
                    "headers": {"Authorization": "Bearer ghp_abc", "X-Org": "acme"}
                },
                "legacy": {"url": "https://legacy.example/sse"},
                "filesystem": {
                    "command": "npx",
                    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/Users/me/My Files"],
                    "env": {"LOG_LEVEL": "debug"}
                }
            }
        }))
        .unwrap();

        let github = servers
            .iter()
            .find(|server| server.name == "github")
            .unwrap();
        assert_eq!(github.transport, "streamable_http");
        assert_eq!(github.headers.len(), 2);
        assert!(github.missing.is_empty());

        let legacy = servers
            .iter()
            .find(|server| server.name == "legacy")
            .unwrap();
        assert_eq!(legacy.transport, "sse");

        let filesystem = servers
            .iter()
            .find(|server| server.name == "filesystem")
            .unwrap();
        assert_eq!(filesystem.transport, "stdio");
        assert_eq!(filesystem.endpoint_url, None);
        assert_eq!(
            filesystem.command.as_deref(),
            Some("npx -y @modelcontextprotocol/server-filesystem '/Users/me/My Files'")
        );
        assert_eq!(filesystem.env[0].value.as_deref(), Some("debug"));
    }

    #[test]
    fn vs_code_variables_are_left_to_fill_in() {
        let servers = parse_client_config(&json!({
            "inputs": [{"id": "token", "type": "promptString", "password": true}],
            "servers": {
                "linear": {
                    "type": "http",
                    "url": "https://mcp.linear.app/mcp",
                    "headers": {"Authorization": "Bearer ${input:token}"}
                }
            }
        }))
        .unwrap();

        assert_eq!(servers[0].missing, vec!["Authorization".to_string()]);
        assert!(servers[0].headers[0].secret);
        assert_eq!(servers[0].headers[0].value, None);
    }

    #[test]
    fn a_bare_map_and_vs_code_settings_are_read() {
        let bare = parse_client_config(&json!({
            "notion": {"url": "https://mcp.notion.com/mcp", "disabled": true}
        }))
        .unwrap();
        assert!(bare[0].disabled);

        let settings = parse_client_config(&json!({
            "mcp": {"servers": {"sentry": {"type": "sse", "url": "https://mcp.sentry.dev/sse"}}}
        }))
        .unwrap();
        assert_eq!(settings[0].transport, "sse");
    }

    #[test]
    fn broken_configurations_say_what_is_wrong() {
        for value in [
            json!({"hello": "world"}),
            json!({"mcpServers": {}}),
            json!({"mcpServers": {"x": {"type": "websocket", "url": "wss://x"}}}),
            json!({"mcpServers": {"x": {"type": "http"}}}),
            json!({"mcpServers": {"x": {"command": "npx", "args": "-y"}}}),
            json!({"mcpServers": {"x": {"url": "https://x", "headers": ["a"]}}}),
        ] {
            assert!(parse_client_config(&value).is_err(), "{value}");
        }
    }
}
