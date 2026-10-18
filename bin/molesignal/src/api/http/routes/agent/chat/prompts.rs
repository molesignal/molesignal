// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一轮回答的 system 与任务 prompt：解析、渲染白名单变量，记下活跃 prompt 的引用与 hash。

use serde_json::{Map, Value};

use super::PostMessageReq;
use crate::{
    agent::{capabilities::dashboard_authoring, tools::AgentExecutionPolicy},
    api::AppState,
    app::iam::IamContext,
    infra::persistence::repositories::agent::{
        chats::Chat,
        prompts::{prompt_hash, render_prompt},
    },
    shared::{ids::Id, time::TimestampMicros},
};

/// 解析出的活跃 prompt 元数据（持久化到 assistant 消息）。
#[derive(Clone, Default)]
pub(super) struct PromptMeta {
    pub(super) id: Option<String>,
    pub(super) builtin_key: Option<String>,
    pub(super) version: Option<i32>,
    pub(super) hash: Option<String>,
}

/// analysis_mode → prompt purpose；自由问答返 None（只用 system prompt）。
fn purpose_for_mode(mode: &str) -> Option<&'static str> {
    match mode {
        "anomaly" | "anomaly_analysis" => Some("anomaly_analysis"),
        "root_cause" | "rootcause" => Some("root_cause"),
        "alert" | "alert_explain" => Some("alert_explain"),
        "query" | "query_generation" => Some("query_generation"),
        "dashboard" | "dashboard_authoring" => Some("dashboard_authoring"),
        _ => None,
    }
}

fn prompt_readable(
    t: &crate::infra::persistence::repositories::agent::prompts::AgentPromptTemplate,
    ctx: &IamContext,
) -> bool {
    match t.scope.as_str() {
        "builtin" => true,
        "org" => t.org_id.as_deref() == Some(ctx.org_id.0.as_str()),
        "user" => {
            t.org_id.as_deref() == Some(ctx.org_id.0.as_str())
                && t.user_id.as_deref() == Some(ctx.user_id.0.as_str())
        }
        _ => false,
    }
}

/// 解析 system + 任务 prompt，渲染白名单变量，返回 (system_texts, 活跃 prompt 元数据)。
/// `alert_name` 是 `/rca` 调查的事件，填进任务 prompt。全程容错：解析/渲染失败不阻断 chat
/// （返回已收集的部分）。
pub(super) async fn resolve_prompts(
    state: &AppState,
    ctx: &IamContext,
    chat: &Chat,
    req: &PostMessageReq,
    alert_name: Option<&str>,
    dashboard_capability_active: bool,
) -> (Vec<String>, PromptMeta) {
    let now = TimestampMicros::now();

    let org_name = state
        .iam
        .service
        .orgs
        .get(&ctx.org_id)
        .await
        .map(|o| o.name)
        .unwrap_or_else(|_| ctx.org_id.0.clone());

    let current_time = now.to_datetime().to_rfc3339();

    let time_range_str = match &req.time_range {
        Some(t) => format!(
            "{} to {}",
            TimestampMicros(t.start_micros).to_datetime().to_rfc3339(),
            TimestampMicros(t.end_micros).to_datetime().to_rfc3339(),
        ),
        None => "unspecified".into(),
    };

    let streams_str = if req.stream_hints.is_empty() {
        "all available streams".to_string()
    } else {
        req.stream_hints.join(", ")
    };

    let mut system_vars = Map::new();
    system_vars.insert("org_name".into(), Value::String(org_name));
    system_vars.insert("current_time".into(), Value::String(current_time));

    let mut task_vars = Map::new();
    task_vars.insert("time_range".into(), Value::String(time_range_str));
    task_vars.insert("streams".into(), Value::String(streams_str));
    task_vars.insert(
        "alert_name".into(),
        Value::String(alert_name.unwrap_or_default().to_string()),
    );

    let mut system_texts = Vec::new();
    let mut active = PromptMeta::default();

    // system prompt（always）。
    if let Ok(sys) = state
        .agent
        .prompts
        .resolve(&ctx.org_id, &ctx.user_id, "system")
        .await
    {
        let rendered = render_prompt(&sys.body, &system_vars);

        active = PromptMeta {
            id: Some(sys.id.0.clone()),
            builtin_key: sys.builtin_key.clone(),
            version: Some(sys.version),
            hash: Some(prompt_hash(&rendered)),
        };

        system_texts.push(rendered);
    }

    system_texts.push(
        match req.execution_policy.unwrap_or_default() {
            AgentExecutionPolicy::AdviceOnly => {
                "The user selected Advice only. You may use read-only investigation tools, but do not create approval requests. Describe any proposed operation without invoking a write-capable tool."
            }
            AgentExecutionPolicy::ReadOnly => {
                "The user selected Auto-run read-only. Run authorized read-only investigation tools as needed, but do not create approval requests or invoke write-capable tools."
            }
            AgentExecutionPolicy::Policy => {
                "The user selected Policy approval. Read-only tools may run automatically. Any state-changing operation must only be proposed through the approval workflow and must never be executed directly."
            }
            AgentExecutionPolicy::Auto => {
                "The user selected Auto. Read-only tools run automatically. A change the organization lets run after the user's confirmation runs without asking when it can be undone; a destructive one still asks. Operations that need approval must be proposed through the approval workflow and never executed directly. Say what you changed."
            }
            AgentExecutionPolicy::BypassPermissions => {
                "The user selected Bypass permissions. Read-only tools run automatically, and a change the organization lets run after the user's confirmation runs without asking, a destructive one included: say exactly what you will change before you change it. Operations that need approval must still be proposed through the approval workflow and never executed directly."
            }
        }
        .to_string(),
    );

    if req.analysis_mode.as_deref() == Some("query_generation") {
        system_texts.push(
            "The user selected Generate query only. Generate the query text, but do not call tools or execute the query."
                .to_string(),
        );
    } else {
        system_texts.push(observability_query_instruction().to_string());
    }

    // 任务 prompt：显式 id → analysis_mode purpose → 无。
    let task = if dashboard_capability_active {
        state
            .agent
            .prompts
            .resolve(&ctx.org_id, &ctx.user_id, "dashboard_authoring")
            .await
            .ok()
    } else if let Some(pid) = req.prompt_template_id.as_deref() {
        match state.agent.prompts.get(&Id(pid.to_string())).await {
            Ok(t) if prompt_readable(&t, ctx) => Some(t),
            _ => None,
        }
    } else if let Some(purpose) = req
        .analysis_mode
        .as_deref()
        .or(chat.analysis_mode.as_deref())
        .and_then(purpose_for_mode)
    {
        state
            .agent
            .prompts
            .resolve(&ctx.org_id, &ctx.user_id, purpose)
            .await
            .ok()
    } else {
        None
    };

    if let Some(t) = task {
        let rendered = render_prompt(&t.body, &task_vars);

        active = PromptMeta {
            id: Some(t.id.0.clone()),
            builtin_key: t.builtin_key.clone(),
            version: Some(t.version),
            hash: Some(prompt_hash(&rendered)),
        };

        system_texts.push(rendered);
    } else if dashboard_capability_active {
        let instruction = dashboard_authoring::INSTRUCTIONS.to_string();

        active = PromptMeta {
            id: None,
            builtin_key: Some("dashboard.authoring.v1".into()),
            version: Some(1),
            hash: Some(prompt_hash(&instruction)),
        };

        system_texts.push(instruction);
    }

    system_texts.push(
        answer_presentation_instruction(req.analysis_mode.as_deref() == Some("query_generation"))
            .to_string(),
    );

    (system_texts, active)
}

fn answer_presentation_instruction(query_generation_only: bool) -> &'static str {
    if query_generation_only {
        return "Write only the requested query plus a short explanation of what it returns. Do not expose internal tool names, function names, parameter names, data-stream discovery attempts, hidden reasoning, or implementation failures.";
    }

    r#"Your final response is a product-facing operations result, not a debug trace. Never expose internal tool/function names, raw parameter names, hidden reasoning, data-stream discovery attempts, or implementation failures. Do not narrate "I first tried..." or similar process language. Return one JSON object without a Markdown fence using this shape: {"summary":"concise conclusion","evidence":[{"label":"user-facing fact","kind":"logs|metrics|trace|alert|schedule|other","route":"optional in-app route"}],"likely_causes":["optional cause"],"limitations":["what could not be confirmed"],"suggested_next_steps":["clear next action"],"related_links":[{"label":"action label","route":"optional in-app route"}],"confidence":"high|medium|low"}. Keep empty sections as empty arrays. A route is a path of this product starting with /, such as /logs, /alerts or /agent/knowledge?path=runbooks/api.md for a knowledge file (add &base=team:<team id> for a team's knowledge); leave route out rather than guess a page. Tool execution transparency is rendered separately by the product from recorded evidence."#
}

fn observability_query_instruction() -> &'static str {
    r#"When investigating observability data, use list_streams and then get_stream_schema before the first log or metric query unless the exact current schema is already present in this chat. SQL must reference only exact fields returned for the selected stream. The query_logs time_range argument already constrains event time, so never invent timestamp, time, or _timestamp fields and never order by one unless that exact field exists in the schema. query_metrics accepts PromQL only; label_values() is a Grafana template helper and is invalid. If a needed capability is not visible, discover it with tool_search and invoke the selected result through tools_call. Do not repeat equivalent empty queries. After at most one corrected retry for a failed query, stop querying that path, explain the limitation in the final answer, and use the evidence already collected to produce a conclusion."#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purpose_mapping() {
        assert_eq!(purpose_for_mode("root_cause"), Some("root_cause"));
        assert_eq!(purpose_for_mode("anomaly"), Some("anomaly_analysis"));
        assert_eq!(purpose_for_mode("freeform"), None);
    }

    #[test]
    fn final_answer_instruction_hides_internal_execution_details() {
        let instruction = answer_presentation_instruction(false);
        assert!(instruction.contains("Never expose internal tool/function names"));
        assert!(instruction.contains("\"limitations\""));
        assert!(instruction.contains("rendered separately"));
    }

    #[test]
    fn observability_query_instruction_prevents_known_query_mistakes() {
        let instruction = observability_query_instruction();
        assert!(instruction.contains("list_streams"));
        assert!(instruction.contains("never invent timestamp"));
        assert!(instruction.contains("label_values()"));
        assert!(instruction.contains("Do not repeat equivalent empty queries"));
    }
}
