// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashSet;

use rmcp::{
    ErrorData, RoleServer,
    model::{
        CacheScope, GetPromptRequestParams, GetPromptResponse, GetPromptResult, ListPromptsResult,
        PaginatedRequestParams, Prompt, PromptArgument, PromptMessage, Role,
    },
    service::RequestContext,
};
use serde_json::{Map, Value};

use super::handler::{InboundMcpHandler, protocol_error, request_data};
use crate::{
    domain::iam::access::IamPrincipalType,
    infra::persistence::repositories::agent::prompts::{
        AgentPromptTemplate, allowed_variables, render_prompt,
    },
};

pub(super) async fn list(
    handler: &InboundMcpHandler,
    _request: Option<PaginatedRequestParams>,
    context: &RequestContext<RoleServer>,
) -> Result<ListPromptsResult, ErrorData> {
    let data = request_data(handler, context)?;
    let templates = visible_templates(handler, &data.iam).await?;
    let prompts = templates
        .into_iter()
        .map(protocol_prompt)
        .collect::<Vec<_>>();
    let response_too_large = match serde_json::to_vec(&prompts) {
        Ok(encoded) => encoded.len() > data.settings.max_response_bytes.max(1) as usize,
        Err(_) => true,
    };
    if response_too_large {
        return Err(ErrorData::invalid_request(
            "prompt list exceeds the configured response limit",
            None,
        ));
    }
    Ok(ListPromptsResult {
        prompts,
        ..Default::default()
    }
    .with_ttl_ms(0)
    .with_cache_scope(CacheScope::Private))
}

pub(super) async fn get(
    handler: &InboundMcpHandler,
    request: GetPromptRequestParams,
    context: &RequestContext<RoleServer>,
) -> Result<GetPromptResponse, ErrorData> {
    if request.request_state.is_some() || request.input_responses.is_some() {
        return Err(ErrorData::invalid_params(
            "this prompt has no active multi-round input request",
            None,
        ));
    }
    let data = request_data(handler, context)?;
    let template = visible_templates(handler, &data.iam)
        .await?
        .into_iter()
        .find(|template| prompt_name(template) == request.name)
        .ok_or_else(|| ErrorData::resource_not_found("prompt was not found", None))?;
    let arguments = request.arguments.unwrap_or_default();
    validate_arguments(&template.variables_schema, &arguments)?;
    let rendered = render_prompt(&template.body, &arguments);
    if rendered.len() > data.settings.max_response_bytes.max(1) as usize {
        return Err(ErrorData::invalid_request(
            "rendered prompt exceeds the configured response limit",
            None,
        ));
    }
    Ok(
        GetPromptResult::new(vec![PromptMessage::new_text(Role::User, rendered)])
            .with_description(format!(
                "{} (purpose: {}, scope: {})",
                template.name, template.purpose, template.scope
            ))
            .into(),
    )
}

async fn visible_templates(
    handler: &InboundMcpHandler,
    iam: &crate::app::iam::IamContext,
) -> Result<Vec<AgentPromptTemplate>, ErrorData> {
    let templates = handler
        .state
        .agent
        .prompts
        .list(&iam.org_id, &iam.user_id)
        .await
        .map_err(protocol_error)?;
    Ok(templates
        .into_iter()
        .filter(|template| template.enabled)
        .filter(|template| match template.scope.as_str() {
            "builtin" => template.org_id.is_none() && template.user_id.is_none(),
            "org" => template.org_id.as_deref() == Some(iam.org_id.as_str()),
            "user" => {
                iam.principal_type() == IamPrincipalType::User
                    && template.org_id.as_deref() == Some(iam.org_id.as_str())
                    && template.user_id.as_deref() == Some(iam.user_id.as_str())
            }
            _ => false,
        })
        .collect())
}

fn protocol_prompt(template: AgentPromptTemplate) -> Prompt {
    Prompt::new(
        prompt_name(&template),
        Some(format!(
            "{} (purpose: {}, scope: {})",
            template.name, template.purpose, template.scope
        )),
        Some(prompt_arguments(&template.variables_schema)),
    )
    .with_title(template.name)
}

fn prompt_name(template: &AgentPromptTemplate) -> String {
    format!("molesignal_prompt_{}", template.id.0)
}

fn prompt_arguments(schema: &Value) -> Vec<PromptArgument> {
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<HashSet<_>>();
    schema
        .get("properties")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|properties| properties.iter())
        .map(|(name, property)| {
            let mut argument =
                PromptArgument::new(name).with_required(required.contains(name.as_str()));
            if let Some(title) = property.get("title").and_then(Value::as_str) {
                argument = argument.with_title(title);
            }
            if let Some(description) = property.get("description").and_then(Value::as_str) {
                argument = argument.with_description(description);
            }
            argument
        })
        .collect()
}

fn validate_arguments(schema: &Value, arguments: &Map<String, Value>) -> Result<(), ErrorData> {
    let allowed = allowed_variables(schema)
        .into_iter()
        .collect::<HashSet<_>>();
    if let Some(unknown) = arguments.keys().find(|name| !allowed.contains(*name)) {
        return Err(ErrorData::invalid_params(
            format!("unknown prompt argument `{unknown}`"),
            None,
        ));
    }
    let required = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str);
    for name in required {
        if !arguments.contains_key(name) {
            return Err(ErrorData::invalid_params(
                format!("missing required prompt argument `{name}`"),
                None,
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn validates_required_and_unknown_prompt_arguments() {
        let schema = json!({
            "type": "object",
            "properties": {"service": {"type": "string"}},
            "required": ["service"]
        });
        assert!(validate_arguments(&schema, &Map::new()).is_err());
        let arguments = serde_json::from_value(json!({"service": "api"})).unwrap();
        assert!(validate_arguments(&schema, &arguments).is_ok());
    }
}
