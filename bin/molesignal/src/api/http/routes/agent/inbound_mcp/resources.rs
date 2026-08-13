// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use rmcp::{
    ErrorData, RoleServer,
    model::{
        CacheScope, ListResourceTemplatesResult, ListResourcesResult, PaginatedRequestParams,
        ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult, Resource,
        ResourceContents, ResourceTemplate,
    },
    service::RequestContext,
};
use serde_json::{Value, json};
use tool_runtime::ToolContent;
use url::Url;

use super::{
    handler::{InboundMcpHandler, protocol_error, request_data},
    tools::{catalog, execution},
};

const CAPABILITIES_URI: &str = "molesignal://platform/capabilities";
const CATALOG_URI: &str = "molesignal://tools/catalog";

pub(super) async fn list(
    handler: &InboundMcpHandler,
    _request: Option<PaginatedRequestParams>,
    context: &RequestContext<RoleServer>,
) -> Result<ListResourcesResult, ErrorData> {
    let _ = request_data(handler, context)?;
    Ok(ListResourcesResult {
        resources: vec![
            Resource::new(CAPABILITIES_URI, "platform_capabilities")
                .with_title("MoleSignal platform capabilities")
                .with_description("Capabilities visible to this authenticated MCP principal.")
                .with_mime_type("application/json"),
            Resource::new(CATALOG_URI, "tool_catalog")
                .with_title("Inbound MCP tool catalog")
                .with_description("Full IAM and Tool Policy filtered Inbound MCP catalog.")
                .with_mime_type("application/json"),
        ],
        ..Default::default()
    }
    .with_ttl_ms(0)
    .with_cache_scope(CacheScope::Private))
}

pub(super) async fn list_templates(
    handler: &InboundMcpHandler,
    _request: Option<PaginatedRequestParams>,
    context: &RequestContext<RoleServer>,
) -> Result<ListResourceTemplatesResult, ErrorData> {
    let data = request_data(handler, context)?;
    let candidates = [
        (
            "molesignal://approvals/{approval_id}",
            "agent_approval",
            "get_agent_approval",
            "One Mole Agent approval request.",
        ),
        (
            "molesignal://executions/{execution_id}",
            "agent_execution",
            "get_agent_execution",
            "One approved-operation execution.",
        ),
        (
            "molesignal://search-jobs/{search_job_id}",
            "search_job",
            "get_search_job",
            "One asynchronous search job.",
        ),
        (
            "molesignal://streams/{stream_type}/{stream_name}/schema",
            "stream_schema",
            "get_stream_schema",
            "The current schema for a telemetry stream.",
        ),
    ];
    let mut templates = Vec::new();
    for (uri, name, tool, description) in candidates {
        if catalog::resolve(handler, &data.iam, tool).await.is_ok() {
            templates.push(
                ResourceTemplate::new(uri, name)
                    .with_description(description)
                    .with_mime_type("application/json"),
            );
        }
    }
    Ok(ListResourceTemplatesResult {
        resource_templates: templates,
        ..Default::default()
    }
    .with_ttl_ms(0)
    .with_cache_scope(CacheScope::Private))
}

pub(super) async fn read(
    handler: &InboundMcpHandler,
    request: ReadResourceRequestParams,
    context: &RequestContext<RoleServer>,
) -> Result<ReadResourceResponse, ErrorData> {
    let data = request_data(handler, context)?;
    let value = match request.uri.as_str() {
        CAPABILITIES_URI => {
            call_read_tool(
                handler,
                context,
                &data,
                "get_platform_capabilities",
                json!({}),
            )
            .await?
        }
        CATALOG_URI => catalog::resource_catalog(handler, &data.iam)
            .await
            .map_err(protocol_error)?,
        uri => {
            let target = parse_dynamic_uri(uri)?;
            call_read_tool(handler, context, &data, target.tool, target.arguments).await?
        }
    };
    let text = serde_json::to_string_pretty(&value)
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
    if text.len() > data.settings.max_response_bytes.max(1) as usize {
        return Err(ErrorData::invalid_request(
            "resource response exceeds the configured response limit",
            None,
        ));
    }
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(text, request.uri).with_mime_type("application/json"),
    ])
    .with_ttl_ms(0)
    .with_cache_scope(CacheScope::Private)
    .into())
}

async fn call_read_tool(
    handler: &InboundMcpHandler,
    context: &RequestContext<RoleServer>,
    data: &super::handler::InboundRequestData,
    name: &str,
    arguments: Value,
) -> Result<Value, ErrorData> {
    let resolved = catalog::resolve(handler, &data.iam, name)
        .await
        .map_err(protocol_error)?;
    let result =
        execution::execute_resolved(handler, data, resolved, arguments, context.ct.child_token())
            .await
            .map_err(protocol_error)?;
    if result.is_error {
        return Err(ErrorData::invalid_request(
            result
                .first_text()
                .unwrap_or_else(|| "resource read failed".into()),
            None,
        ));
    }
    let values = result
        .content
        .into_iter()
        .map(|content| match content {
            ToolContent::Text { text } => Value::String(text),
            ToolContent::Json { json } => json,
        })
        .collect::<Vec<_>>();
    Ok(match values.len() {
        0 => Value::Null,
        1 => values.into_iter().next().unwrap_or(Value::Null),
        _ => json!({ "items": values }),
    })
}

struct DynamicResource {
    tool: &'static str,
    arguments: Value,
}

fn parse_dynamic_uri(uri: &str) -> Result<DynamicResource, ErrorData> {
    if uri.len() > 4_096 {
        return Err(ErrorData::resource_not_found(
            "MoleSignal resource URI exceeds 4096 bytes",
            None,
        ));
    }
    let parsed = Url::parse(uri)
        .map_err(|_| ErrorData::resource_not_found("invalid MoleSignal resource URI", None))?;
    if parsed.scheme() != "molesignal" || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err(ErrorData::resource_not_found(
            "unsupported MoleSignal resource URI",
            None,
        ));
    }
    let host = parsed.host_str().unwrap_or_default();
    let segments = parsed
        .path_segments()
        .into_iter()
        .flatten()
        .map(decode_segment)
        .collect::<Result<Vec<_>, _>>()?;
    match (host, segments.as_slice()) {
        ("approvals", [id]) => Ok(DynamicResource {
            tool: "get_agent_approval",
            arguments: json!({"approval_id": id}),
        }),
        ("executions", [id]) => Ok(DynamicResource {
            tool: "get_agent_execution",
            arguments: json!({"execution_id": id}),
        }),
        ("search-jobs", [id]) => Ok(DynamicResource {
            tool: "get_search_job",
            arguments: json!({"job_id": id}),
        }),
        ("streams", [stream_type, name, suffix])
            if suffix == "schema"
                && matches!(
                    stream_type.as_str(),
                    "logs" | "metrics" | "traces" | "profiles" | "extend"
                ) =>
        {
            Ok(DynamicResource {
                tool: "get_stream_schema",
                arguments: json!({"stream": name, "stream_type": stream_type}),
            })
        }
        _ => Err(ErrorData::resource_not_found(
            format!("resource `{uri}` was not found"),
            None,
        )),
    }
}

fn decode_segment(segment: &str) -> Result<String, ErrorData> {
    percent_encoding::percent_decode_str(segment)
        .decode_utf8()
        .map(|value| value.into_owned())
        .map_err(|_| ErrorData::resource_not_found("resource URI contains invalid UTF-8", None))
        .and_then(|value| {
            if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
                Err(ErrorData::resource_not_found(
                    "resource URI segment is invalid",
                    None,
                ))
            } else {
                Ok(value)
            }
        })
}

pub(super) fn is_subscribable(uri: &str) -> bool {
    matches!(uri, CAPABILITIES_URI | CATALOG_URI) || parse_dynamic_uri(uri).is_ok()
}

pub(super) async fn authorize_subscription(
    handler: &InboundMcpHandler,
    iam: &crate::app::iam::IamContext,
    uri: &str,
) -> Result<(), ErrorData> {
    if matches!(uri, CAPABILITIES_URI | CATALOG_URI) {
        return Ok(());
    }
    let target = parse_dynamic_uri(uri)?;
    catalog::resolve(handler, iam, target.tool)
        .await
        .map(|_| ())
        .map_err(protocol_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tenant_safe_resource_templates_without_org_arguments() {
        let resource = parse_dynamic_uri("molesignal://streams/logs/app%2Flogs/schema")
            .expect("stream resource");
        assert_eq!(resource.tool, "get_stream_schema");
        assert_eq!(resource.arguments["stream"], "app/logs");
        assert!(parse_dynamic_uri("molesignal://streams/x/schema?org=other").is_err());
    }
}
