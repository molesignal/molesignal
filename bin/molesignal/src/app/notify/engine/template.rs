// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{
    NotifyEngine,
    blocks::{resolve_blocks, validate_blocks},
};
use crate::{
    domain::notify::{
        connector::{NotifyChannel, NotifyMessage},
        policy::{NotifyEvent, NotifyPolicy},
        preference::NotifyCategory,
        template::{NotifyCard, NotifyCardField, NotifyCardLink, NotifyTemplate, ProviderTemplate},
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 模板能写成的格式；`json` 只给 Webhook 渠道的模板，变量按 JSON 字符串转义后填入。
const FORMATS: &[&str] = &["text", "markdown", "html", "json"];

/// 一个模板最多写给几个服务商。
const MAX_PROVIDERS: usize = 8;

/// 服务商的模板最多几个变量。
const MAX_PARAMS: usize = 20;

/// 一个变量的写法最长多少个字。
const MAX_PARAM_CHARS: usize = 500;

/// 一张卡片最多几个字段、几个链接按钮。
const MAX_CARD_FIELDS: usize = 10;
const MAX_CARD_LINKS: usize = 5;

/// 一个通知策略发出去的内容：每个渠道用策略给它另选的模板，没选的用通用模板，都没有时照通知本身。
pub(super) struct PolicyMessages {
    message: NotifyMessage,
    general: Option<NotifyTemplate>,
    channels: BTreeMap<NotifyChannel, NotifyTemplate>,
}

impl PolicyMessages {
    /// 发往 `channel` 的那一份。
    pub(super) fn for_channel(
        &self,
        channel: NotifyChannel,
        event: &NotifyEvent,
    ) -> Result<NotifyMessage> {
        match self.channels.get(&channel).or(self.general.as_ref()) {
            Some(template) => render_message(template, event, &self.message),
            None => Ok(self.message.clone()),
        }
    }
}

/// 通知策略给 `channel` 用哪个模板：另选的，没有时是通用的。
pub(super) fn policy_template_id(policy: &NotifyPolicy, channel: NotifyChannel) -> Option<&Id> {
    policy
        .channel_templates
        .get(&channel)
        .or(policy.template_id.as_ref())
}

impl NotifyEngine {
    /// 读出通知策略用的模板，备好这条通知发往各渠道的样子。
    pub(super) async fn policy_messages(
        &self,
        policy: &NotifyPolicy,
        event: &NotifyEvent,
        message: &NotifyMessage,
    ) -> Result<PolicyMessages> {
        let general = match &policy.template_id {
            Some(id) => Some(self.notify_template(&event.organization_id, id).await?),
            None => None,
        };

        let mut channels = BTreeMap::new();

        for (channel, id) in &policy.channel_templates {
            channels.insert(
                *channel,
                self.notify_template(&event.organization_id, id).await?,
            );
        }

        Ok(PolicyMessages {
            message: message.clone(),
            general,
            channels,
        })
    }

    pub(super) async fn notify_template(
        &self,
        organization_id: &Id,
        id: &Id,
    ) -> Result<NotifyTemplate> {
        let template = self.templates.get(organization_id, id).await?;
        validate_notify_template_body(&template.body)?;
        Ok(template)
    }
}

pub fn validate_notify_template_body(body: &str) -> Result<()> {
    if body.trim().is_empty() {
        return Err(Error::invalid("notify template body must not be empty"));
    }

    validate_placeholders(body)
}

fn validate_placeholders(value: &str) -> Result<()> {
    if let Some(path) = placeholder_paths(value).find(|path| path.len() > 128) {
        return Err(Error::invalid(format!(
            "notify template placeholder is too long: {path}"
        )));
    }

    validate_blocks(value)
}

/// JSON 模板填好之后要是一个合法的 JSON：保存时每个变量先按 `0` 填（放在引号里是字符串，不放是
/// 数字），看结构对不对；发送时再按真实的值检查一次。
fn validate_json_body(body: &str) -> Result<()> {
    let filled = fill_with(body, &BTreeMap::new(), |_| Some("0".into()));

    serde_json::from_str::<Value>(&filled)
        .map(drop)
        .map_err(|error| Error::invalid(format!("a JSON template must be valid JSON: {error}")))
}

/// 存模板前的检查：正文与格式；短信与电话模板只写纯文本，也只有它们写给服务商的模板；只有即时通讯模板
/// 有卡片。
pub fn validate_notify_template(
    channel: Option<NotifyChannel>,
    format: &str,
    body: &str,
    provider_templates: &BTreeMap<String, ProviderTemplate>,
    card: Option<&NotifyCard>,
) -> Result<()> {
    validate_notify_template_body(body)?;

    if !FORMATS.contains(&format) {
        return Err(Error::invalid(
            "format must be one of: text | markdown | html | json",
        ));
    }

    if format == "json" {
        if channel != Some(NotifyChannel::Webhook) {
            return Err(Error::invalid("only webhook templates are written in JSON"));
        }

        validate_json_body(body)?;
    }

    if let Some(card) = card {
        if channel != Some(NotifyChannel::Im) {
            return Err(Error::invalid(
                "only instant messaging templates carry a card",
            ));
        }

        validate_card(card)?;
    }

    let plain_text = channel.is_some_and(NotifyChannel::plain_text);

    if plain_text && format != "text" {
        return Err(Error::invalid(
            "SMS and phone templates are plain text: their format is text",
        ));
    }

    if provider_templates.is_empty() {
        return Ok(());
    }

    if !plain_text {
        return Err(Error::invalid(
            "only SMS and phone templates carry templates approved by a provider",
        ));
    }

    if provider_templates.len() > MAX_PROVIDERS {
        return Err(Error::invalid(format!(
            "a notify template carries at most {MAX_PROVIDERS} provider templates"
        )));
    }

    for (provider, template) in provider_templates {
        validate_provider_template(provider, template)?;
    }

    Ok(())
}

/// 卡片的标题与页脚不长；字段要有名称和值；链接按钮要有文字，地址是 http(s) 的，或者是填出这种地址的
/// 占位符。
fn validate_card(card: &NotifyCard) -> Result<()> {
    let text = |value: &str, max: usize, what: &str| {
        if value.chars().count() > max {
            return Err(Error::invalid(format!(
                "the card {what} is longer than {max} characters"
            )));
        }

        validate_placeholders(value)
    };

    let filled = |value: &str, max: usize, what: &str| {
        if value.trim().is_empty() {
            return Err(Error::invalid(format!("the card {what} must not be empty")));
        }

        text(value, max, what)
    };

    text(&card.title, 200, "title")?;
    text(&card.note, 200, "note")?;

    if card.fields.len() > MAX_CARD_FIELDS {
        return Err(Error::invalid(format!(
            "a card has at most {MAX_CARD_FIELDS} fields"
        )));
    }

    for field in &card.fields {
        filled(&field.label, 40, "field name")?;
        filled(&field.value, 500, "field value")?;
    }

    if card.links.len() > MAX_CARD_LINKS {
        return Err(Error::invalid(format!(
            "a card has at most {MAX_CARD_LINKS} links"
        )));
    }

    for link in &card.links {
        filled(&link.label, 30, "link text")?;
        filled(&link.url, 2_000, "link address")?;

        let url = link.url.trim();

        if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("{{")) {
            return Err(Error::invalid(
                "a card link goes to an http(s) address, or a placeholder that fills one",
            ));
        }
    }

    Ok(())
}

/// 服务商的名字是小写的一个词；模板编号是服务商给的那串字母数字；每个变量都要写上填什么。
fn validate_provider_template(provider: &str, template: &ProviderTemplate) -> Result<()> {
    let word = |value: &str, max: usize| {
        (1..=max).contains(&value.len())
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    };

    if !word(provider, 32) || provider.chars().any(|c| c.is_ascii_uppercase()) {
        return Err(Error::invalid(format!(
            "provider template name is not valid: {provider}"
        )));
    }

    if !word(template.code.trim(), 64) {
        return Err(Error::invalid(format!(
            "the {provider} template code is the one the provider gave, such as SMS_123456789"
        )));
    }

    if template.params.len() > MAX_PARAMS {
        return Err(Error::invalid(format!(
            "the {provider} template has more than {MAX_PARAMS} variables"
        )));
    }

    for (name, value) in &template.params {
        if !word(name, 64) || name.contains('-') {
            return Err(Error::invalid(format!(
                "the {provider} template variable {name} is not a variable name, such as content"
            )));
        }

        if value.trim().is_empty() {
            return Err(Error::invalid(format!(
                "the {provider} template variable {name} needs a value"
            )));
        }

        if value.chars().count() > MAX_PARAM_CHARS {
            return Err(Error::invalid(format!(
                "the {provider} template variable {name} is longer than {MAX_PARAM_CHARS} characters"
            )));
        }

        validate_placeholders(value)?;
    }

    if template
        .max_param_chars
        .is_some_and(|max| !(1..=500).contains(&max))
    {
        return Err(Error::invalid(format!(
            "the {provider} template max_param_chars must be between 1 and 500"
        )));
    }

    Ok(())
}

/// 套用模板：正文按格式成为通知的内容，写给服务商的模板逐个变量填好，卡片的各处也填好。电话与短信模板
/// 的正文就是发出去的全部内容，不再带通知的标题。
fn render_message(
    template: &NotifyTemplate,
    event: &NotifyEvent,
    message: &NotifyMessage,
) -> Result<NotifyMessage> {
    let values = template_values(event, message);
    let json = template.format == "json";

    let body = if json {
        let body = fill_with(&template.body, &values, |value| value.map(json_escaped));

        serde_json::from_str::<Value>(&body).map_err(|error| {
            Error::invalid(format!(
                "the webhook template did not fill into valid JSON: {error}"
            ))
        })?;

        body
    } else {
        fill(&template.body, &values)
    };

    let title = if template.channel.is_some_and(NotifyChannel::plain_text) {
        String::new()
    } else {
        message.title.clone()
    };

    let (markdown, html) = match template.format.as_str() {
        "text" | "json" => (None, None),
        "markdown" => (Some(body.clone()), None),
        "html" => (None, Some(body.clone())),
        _ => return Err(Error::invalid("notify template format is not supported")),
    };

    let mut metadata = message.metadata.clone();

    if json {
        metadata.insert("body_format".into(), "json".into());
    }

    let provider_templates = template
        .provider_templates
        .iter()
        .map(|(provider, approved)| {
            let params = approved
                .params
                .iter()
                .map(|(name, value)| (name.clone(), fill(value, &values)))
                .collect();

            let filled = ProviderTemplate {
                code: approved.code.trim().to_string(),
                params,
                max_param_chars: approved.max_param_chars,
            };

            (provider.clone(), filled)
        })
        .collect();

    let card = template
        .card
        .as_ref()
        .map(|card| render_card(card, &values));

    Ok(NotifyMessage {
        title,
        text: body,
        markdown,
        html,
        metadata,
        provider_templates,
        event: None,
        card,
    })
}

/// 填好卡片：标题与页脚照常填；字段与链接要每个占位符都填得出来才留下，链接还要是 http(s) 地址。
fn render_card(card: &NotifyCard, values: &BTreeMap<String, String>) -> NotifyCard {
    let fields = card
        .fields
        .iter()
        .filter_map(|field| {
            let label = fill_all(&field.label, values)?;
            let value = fill_all(&field.value, values)?;

            (!label.is_empty() && !value.is_empty()).then_some(NotifyCardField { label, value })
        })
        .collect();

    let links = card
        .links
        .iter()
        .filter_map(|link| {
            let label = fill_all(&link.label, values)?;
            let url = fill_all(&link.url, values)?;
            let web = url.starts_with("https://") || url.starts_with("http://");

            (!label.is_empty() && web).then_some(NotifyCardLink { label, url })
        })
        .collect();

    NotifyCard {
        title: fill(&card.title, values).trim().to_string(),
        color: card.color,
        fields,
        links,
        note: fill(&card.note, values).trim().to_string(),
    }
}

/// 填好每个占位符，去掉两头的空白；有填不出来的就是 `None`。
fn fill_all(value: &str, values: &BTreeMap<String, String>) -> Option<String> {
    placeholder_paths(value)
        .all(|path| values.contains_key(path))
        .then(|| fill(value, values).trim().to_string())
}

/// 测试发送时套用模板：用示例事件填占位符，与模板编辑器里的预览一样。
pub(in crate::app::notify) fn render_sample(
    template: &NotifyTemplate,
    organization_id: &Id,
) -> Result<NotifyMessage> {
    let (event, message) = sample(organization_id, template.category);
    render_message(template, &event, &message)
}

/// 一个类别的示例事件与它的标题、正文。
fn sample(organization_id: &Id, category: NotifyCategory) -> (NotifyEvent, NotifyMessage) {
    // 告警五分钟前开始，与模板编辑器里的预览一样。
    let alert = json!({
        "alert_id": "incident-preview",
        "rule_id": "rule-preview",
        "rule_name": "API unavailable",
        "rule": {"description": "The API answers fewer than 99% of requests."},
        "severity": "critical",
        "status": "open",
        "summary": "API unavailable",
        "fingerprint": "api-unavailable",
        "value": 512,
        "threshold": 500,
        "evaluated_at": 1_785_283_200_000_000_i64,
        "started_at": 1_785_282_900_000_000_i64,
        "incident_url": "https://molesignal.example.com/alerts/incidents/incident-preview",
        "service": "api",
        "environment": "production",
        "affected_services": ["api", "checkout"],
        "assignee_user_ids": ["alice"],
        "labels": {"service": "api", "env": "production", "environment": "production"},
        "annotations": {"runbook_url": "https://runbooks.example.com/api-unavailable"},
    });

    let mut escalated = alert.clone();
    escalated["step_index"] = json!(1);
    escalated["loop_index"] = json!(0);
    escalated["target_kind"] = json!("schedule");
    escalated["escalation_reason"] = json!("step");

    let (event_type, attributes, title, text) = match category {
        NotifyCategory::Alert => (
            "alert.triggered",
            alert,
            "CRITICAL · API unavailable",
            "Alert API unavailable is open.",
        ),
        NotifyCategory::Escalation => (
            "alert.escalated",
            escalated,
            "ESCALATION · API unavailable",
            "Alert API unavailable requires escalation.",
        ),
        NotifyCategory::Oncall => (
            "oncall.shift.starting",
            json!({
                "schedule_id": "schedule-preview",
                "schedule_name": "Primary",
                "team_id": "platform",
                "timezone": "Asia/Shanghai",
                "current_user_id": "alice",
                "next_user_id": "bob",
                "transition_at": 1_785_285_000_000_000_i64,
                "override_id": "override-preview",
                "override_user_id": "bob",
                "original_user_id": "alice",
                "start_at": 1_785_283_200_000_000_i64,
                "end_at": 1_785_290_400_000_000_i64,
                "reason": "Temporary coverage",
            }),
            "On-call shift starting soon",
            "Primary changes on-call coverage.",
        ),
        NotifyCategory::Report => (
            "report.ready",
            json!({
                "report_id": "report-preview",
                "report_name": "Weekly reliability report",
                "period_start": "2026-07-20",
                "period_end": "2026-07-26",
                "download_url": "https://reports.example.com/weekly-reliability",
            }),
            "Weekly reliability report is ready",
            "The scheduled report was generated successfully.",
        ),
        NotifyCategory::Security => (
            "security.access.detected",
            json!({
                "action": "Delete API token",
                "actor": "alice@example.com",
                "resource": "api-token/production",
                "ip_address": "203.0.113.42",
            }),
            "SECURITY · Suspicious access detected",
            "A privileged action was observed from a new IP address.",
        ),
        NotifyCategory::System => (
            "system.health.changed",
            json!({
                "component": "query-gateway",
                "status": "degraded",
                "region": "cn-south-1",
            }),
            "System component is degraded",
            "Query latency is above the expected operating range.",
        ),
    };

    let event = NotifyEvent {
        id: format!("{event_type}:preview"),
        event_type: event_type.into(),
        organization_id: organization_id.clone(),
        occurred_at: TimestampMicros(1_785_283_200_000_000),
        attributes,
    };

    let message = NotifyMessage {
        title: title.into(),
        text: text.into(),
        markdown: None,
        html: None,
        metadata: BTreeMap::new(),
        provider_templates: BTreeMap::new(),
        event: None,
        card: None,
    };

    (event, message)
}

/// 正文里的占位符；条件块的 `{{#if …}}`、`{{else}}`、`{{/if}}` 不算。
fn placeholder_paths(body: &str) -> impl Iterator<Item = &str> {
    body.split("{{").skip(1).filter_map(|tail| {
        tail.split_once("}}")
            .map(|(path, _)| path.trim())
            .filter(|path| {
                !path.is_empty()
                    && !path.starts_with('#')
                    && !path.starts_with('/')
                    && *path != "else"
            })
    })
}

fn collect_attribute_values(values: &mut BTreeMap<String, String>, value: &Value, prefix: &str) {
    let Some(fields) = value.as_object() else {
        return;
    };

    for (key, value) in fields {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };

        if value.is_object() {
            collect_attribute_values(values, value, &path);
        } else {
            let rendered = scalar(value);
            values.insert(path.clone(), rendered.clone());
            values.insert(format!("event.attributes.{path}"), rendered);
        }
    }
}

fn alias(values: &mut BTreeMap<String, String>, target: &str, sources: &[&str]) {
    if values.contains_key(target) {
        return;
    }

    if let Some(value) = sources
        .iter()
        .find_map(|source| values.get(*source))
        .cloned()
    {
        values.insert(target.into(), value);
    }
}

fn template_values(event: &NotifyEvent, message: &NotifyMessage) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    values.insert("event.id".into(), event.id.clone());
    values.insert("event.type".into(), event.event_type.clone());
    values.insert("event.occurred_at".into(), event.occurred_at.0.to_string());
    values.insert("message.title".into(), message.title.clone());
    values.insert("message.text".into(), message.text.clone());
    collect_attribute_values(&mut values, &event.attributes, "");

    alias(&mut values, "occurred_at", &["event.occurred_at"]);
    alias(&mut values, "rule.id", &["rule_id"]);

    alias(
        &mut values,
        "rule.name",
        &["rule_name", "summary", "incident.summary"],
    );

    alias(&mut values, "rule.description", &["rule_description"]);
    values.entry("rule.description".into()).or_default();
    alias(&mut values, "incident.id", &["incident_id", "alert_id"]);
    alias(&mut values, "incident.fingerprint", &["fingerprint"]);
    alias(&mut values, "incident.status", &["status"]);
    alias(&mut values, "incident.summary", &["summary"]);

    alias(&mut values, "evaluated_at", &["event.occurred_at"]);

    alias(&mut values, "rule_name", &["rule.name", "summary"]);
    alias(&mut values, "incident_id", &["incident.id", "alert_id"]);

    values
        .entry("value".into())
        .or_insert_with(|| "null".into());

    values
        .entry("threshold".into())
        .or_insert_with(|| "null".into());

    alias(&mut values, "schedule.id", &["schedule_id"]);
    alias(&mut values, "schedule.name", &["schedule_name"]);
    alias(&mut values, "schedule.team_id", &["team_id"]);
    alias(&mut values, "schedule.timezone", &["timezone"]);
    alias(&mut values, "oncall.current_user_id", &["current_user_id"]);
    alias(&mut values, "oncall.next_user_id", &["next_user_id"]);

    alias(&mut values, "oncall.transition_at", &["transition_at"]);

    alias(&mut values, "override.id", &["override_id"]);
    alias(&mut values, "override.user_id", &["override_user_id"]);

    alias(
        &mut values,
        "override.original_user_id",
        &["original_user_id"],
    );

    alias(&mut values, "override.start_at", &["start_at"]);
    alias(&mut values, "override.end_at", &["end_at"]);
    alias(&mut values, "override.reason", &["reason"]);

    alias(&mut values, "incident.url", &["incident_url"]);
    alias(
        &mut values,
        "incident.acknowledged_by",
        &["acknowledged_by"],
    );
    alias(&mut values, "incident.resolved_by", &["resolved_by"]);
    alias(&mut values, "incident.assignees", &["assignee_user_ids"]);
    alias(&mut values, "escalation.target", &["target_kind"]);
    alias(&mut values, "escalation.reason", &["escalation_reason"]);

    add_readable_values(&mut values, event);
    values
}

/// 写给人看的值：时间（UTC）与告警持续的时长，全部标签，升级到第几层、第几轮（都从 1 数起）。`_at`
/// 是微秒，`_time` 是可读的时间。
fn add_readable_values(values: &mut BTreeMap<String, String>, event: &NotifyEvent) {
    let attributes = &event.attributes;
    let micros = |key: &str| attributes.get(key).and_then(Value::as_i64);

    values
        .entry("event.time".into())
        .or_insert_with(|| readable_time(event.occurred_at.0));

    for (target, source) in [
        ("incident.started_time", "started_at"),
        ("incident.acknowledged_time", "acknowledged_at"),
        ("incident.resolved_time", "resolved_at"),
        ("oncall.transition_time", "transition_at"),
        ("override.start_time", "start_at"),
        ("override.end_time", "end_at"),
    ] {
        if let Some(at) = micros(source) {
            values
                .entry(target.into())
                .or_insert_with(|| readable_time(at));
        }
    }

    // 恢复了的告警算到恢复为止，其余算到这件事发生时。
    if let Some(started) = micros("started_at") {
        let ended = micros("resolved_at").unwrap_or(event.occurred_at.0);

        values
            .entry("incident.duration".into())
            .or_insert_with(|| duration_text(ended.saturating_sub(started)));
    }

    if let Some(Value::Object(labels)) = attributes.get("labels") {
        let written = labels
            .iter()
            .map(|(key, value)| format!("{key}={}", scalar(value)))
            .collect::<Vec<_>>()
            .join(", ");

        values.entry("labels".into()).or_insert(written);
    }

    for (target, source) in [
        ("escalation.step", "step_index"),
        ("escalation.loop", "loop_index"),
    ] {
        if let Some(index) = attributes.get(source).and_then(Value::as_u64) {
            values
                .entry(target.into())
                .or_insert_with(|| index.saturating_add(1).to_string());
        }
    }
}

/// 微秒时间写成给人看的样子（UTC），如 2026-10-26 08:24:00 UTC。
fn readable_time(micros: i64) -> String {
    chrono::DateTime::from_timestamp_micros(micros)
        .map(|at| at.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| micros.to_string())
}

/// 一段时长写成最大的两个单位，如 1h 5m、4m 20s；不到一秒是 0s。
fn duration_text(micros: i64) -> String {
    const UNITS: [(u64, &str); 4] = [(86_400, "d"), (3_600, "h"), (60, "m"), (1, "s")];

    let secs = u64::try_from(micros / 1_000_000).unwrap_or(0);

    let Some(first) = UNITS.iter().position(|(size, _)| secs >= *size) else {
        return "0s".into();
    };

    let (size, unit) = UNITS[first];
    let mut text = format!("{}{unit}", secs / size);

    if let Some((next, next_unit)) = UNITS.get(first + 1) {
        let rest = secs % size / next;

        if rest > 0 {
            text.push_str(&format!(" {rest}{next_unit}"));
        }
    }

    text
}

/// 一个属性值写进模板的样子：字符串照原样，数字去掉多余的小数（500.0 写成 500），数组用「, 」连起来，其余
/// 写成 JSON。
fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => match number.as_f64() {
            Some(float) if number.is_f64() && float.fract() == 0.0 && float.abs() < 1e15 => {
                format!("{float:.0}")
            }
            _ => number.to_string(),
        },
        Value::Array(items) => items.iter().map(scalar).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    }
}

fn fill(body: &str, values: &BTreeMap<String, String>) -> String {
    fill_with(body, values, |value| value.map(str::to_string))
}

/// 值写进 JSON 字符串里的样子：转义引号、反斜杠与控制字符，不带两头的引号。
fn json_escaped(value: &str) -> String {
    let quoted = Value::String(value.to_string()).to_string();
    quoted[1..quoted.len() - 1].to_string()
}

/// 先按条件块去掉不成立的分支，再把每个占位符换成 `write` 给出的样子（拿到的是它的值，没有值时是
/// `None`）；`write` 不给时原样留下。
fn fill_with(
    body: &str,
    values: &BTreeMap<String, String>,
    write: impl Fn(Option<&str>) -> Option<String>,
) -> String {
    let resolved = resolve_blocks(body, values);
    let body = resolved.as_str();
    let mut output = String::with_capacity(body.len());
    let mut rest = body;

    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];

        let Some(end) = after_open.find("}}") else {
            output.push_str(&rest[start..]);
            return output;
        };

        let raw_path = &after_open[..end];
        let path = raw_path.trim();

        if let Some(value) = write(values.get(path).map(String::as_str)) {
            output.push_str(&value);
        } else {
            output.push_str("{{");
            output.push_str(raw_path);
            output.push_str("}}");
        }

        rest = &after_open[end + 2..];
    }

    output.push_str(rest);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::notify::template::{
            NOTIFY_TEMPLATE_PRESET_LANGUAGE_PREFIX, NotifyCardColor, NotifyTemplatePreset,
            notify_template_field_catalog, notify_template_preset_catalog, notify_template_presets,
        },
        shared::i18n_language_seed::seeded_pack,
    };

    fn message() -> NotifyMessage {
        NotifyMessage {
            title: "Title".into(),
            text: "Text".into(),
            markdown: None,
            html: None,
            metadata: BTreeMap::new(),
            provider_templates: BTreeMap::new(),
            event: None,
            card: None,
        }
    }

    fn catalog_path(token: &str, category: NotifyCategory) -> String {
        let path = token
            .strip_prefix("{{")
            .and_then(|value| value.strip_suffix("}}"))
            .expect("catalog token");
        match path {
            "labels.<key>" => "labels.service".into(),
            "annotations.<key>" => "annotations.runbook_url".into(),
            "event.attributes.<key>"
                if matches!(category, NotifyCategory::Alert | NotifyCategory::Escalation) =>
            {
                "event.attributes.summary".into()
            }
            "event.attributes.<key>" => "event.attributes.schedule_name".into(),
            _ => path.into(),
        }
    }

    #[test]
    fn catalog_fields_are_renderable_for_alert_escalation_and_oncall() {
        let alert = serde_json::json!({
            "rule_id": "rule",
            "rule_name": "CPU high",
            "rule_description": "CPU threshold exceeded",
            "alert_id": "incident",
            "fingerprint": "cpu-high",
            "status": "resolved",
            "summary": "CPU high",
            "severity": "critical",
            "urgency": "high",
            "priority": "P2",
            "urgency_name": "",
            "priority_name": "Major",
            "value": 90,
            "threshold": 80,
            "evaluated_at": 2,
            "started_at": 1,
            "acknowledged_at": 2,
            "resolved_at": 3,
            "acknowledged_by": "alice",
            "resolved_by": "bob",
            "assignee_user_ids": ["alice"],
            "incident_url": "https://example.test/alerts/incidents/incident",
            "service": "api",
            "environment": "production",
            "affected_services": ["api"],
            "labels": {"service": "api"},
            "annotations": {"runbook_url": "https://example.test"}
        });

        let mut escalated = alert.clone();
        escalated["step_index"] = serde_json::json!(0);
        escalated["loop_index"] = serde_json::json!(0);
        escalated["target_kind"] = serde_json::json!("schedule");
        escalated["escalation_reason"] = serde_json::json!("step");

        let fixtures = [
            (NotifyCategory::Alert, alert),
            (NotifyCategory::Escalation, escalated),
            (
                NotifyCategory::Oncall,
                serde_json::json!({
                    "schedule_id": "schedule",
                    "schedule_name": "Primary",
                    "team_id": "team",
                    "timezone": "UTC",
                    "current_user_id": "alice",
                    "next_user_id": "bob",
                    "transition_at": 2,
                    "override_id": "override",
                    "override_user_id": "bob",
                    "original_user_id": "alice",
                    "start_at": 1,
                    "end_at": 3,
                    "reason": "handoff"
                }),
            ),
        ];

        for (category, attributes) in fixtures {
            let event = NotifyEvent {
                id: "event:1".into(),
                event_type: category.as_str().into(),
                organization_id: Id::new(),
                occurred_at: TimestampMicros(1),
                attributes,
            };
            let values = template_values(&event, &message());
            for field in notify_template_field_catalog()
                .into_iter()
                .filter(|field| field.categories.contains(&category))
                // 回复代码在发出收回复的短信时才填（`NotifyEngine::with_reply`）。
                .filter(|field| !field.token.starts_with("{{reply."))
            {
                let path = catalog_path(&field.token, category);
                assert!(
                    values.contains_key(&path),
                    "catalog field {} is not renderable for {}",
                    field.token,
                    category.as_str()
                );
            }
        }
    }

    #[test]
    fn renders_notify_event_attributes() {
        let rendered = render_message(
            &NotifyTemplate {
                id: Id::new(),
                organization_id: Id::new(),
                category: crate::domain::notify::preference::NotifyCategory::Alert,
                channel: None,
                body: "[{{severity}}] {{summary}} · {{fingerprint}} · {{labels.service}} @ {{event.occurred_at}}".into(),
                format: "markdown".into(),
                provider_templates: BTreeMap::new(),
                card: None,
            },
            &NotifyEvent {
                id: "alert.triggered:1".into(),
                event_type: "alert.triggered".into(),
                organization_id: Id::new(),
                occurred_at: TimestampMicros(1),
                attributes: serde_json::json!({
                    "severity": "critical",
                    "summary": "API unavailable",
                    "fingerprint": "api-unavailable",
                    "labels": {"service": "api"}
                }),
            },
            &NotifyMessage {
                title: "Alert".into(),
                text: "fallback".into(),
                markdown: None,
                html: None,
                metadata: BTreeMap::new(),
                provider_templates: BTreeMap::new(),
                event: None,
                card: None,
            },
        )
        .unwrap();
        assert_eq!(
            rendered.markdown.as_deref(),
            Some("[critical] API unavailable · api-unavailable · api @ 1")
        );
    }

    #[test]
    fn renders_alert_placeholders() {
        let template = NotifyTemplate {
            id: Id::new(),
            organization_id: Id::new(),
            category: crate::domain::notify::preference::NotifyCategory::Alert,
            channel: None,
            body: "{{rule.name}} {{incident.summary}} {{evaluated_at}}".into(),
            format: "text".into(),
            provider_templates: BTreeMap::new(),
            card: None,
        };
        let rendered = render_message(
            &template,
            &NotifyEvent {
                id: "alert.triggered:1".into(),
                event_type: "alert.triggered".into(),
                organization_id: Id::new(),
                occurred_at: TimestampMicros(1),
                attributes: serde_json::json!({
                    "rule_id": "rule",
                    "summary": "CPU high",
                    "status": "open",
                    "fingerprint": "cpu-high"
                }),
            },
            &NotifyMessage {
                title: "Alert".into(),
                text: "fallback".into(),
                markdown: None,
                html: None,
                metadata: BTreeMap::new(),
                provider_templates: BTreeMap::new(),
                event: None,
                card: None,
            },
        )
        .unwrap();

        assert_eq!(rendered.text, "CPU high CPU high 1");
    }

    #[test]
    fn renders_oncall_placeholders_and_whitespace() {
        let rendered = render_message(
            &NotifyTemplate {
                id: Id::new(),
                organization_id: Id::new(),
                category: crate::domain::notify::preference::NotifyCategory::Oncall,
                channel: None,
                body: "{{ schedule.name }} {{oncall.current_user_id}} -> {{oncall.next_user_id}} @ {{oncall.transition_at}}".into(),
                format: "text".into(),
                provider_templates: BTreeMap::new(),
                card: None,
            },
            &NotifyEvent {
                id: "oncall.shift.starting:1".into(),
                event_type: "oncall.shift.starting".into(),
                organization_id: Id::new(),
                occurred_at: TimestampMicros(1),
                attributes: serde_json::json!({
                    "schedule_name": "Primary",
                    "current_user_id": "alice",
                    "next_user_id": "bob",
                    "transition_at": 2
                }),
            },
            &NotifyMessage {
                title: "On-call shift starting".into(),
                text: "fallback".into(),
                markdown: None,
                html: None,
                metadata: BTreeMap::new(),
                provider_templates: BTreeMap::new(),
                event: None,
                card: None,
            },
        )
        .unwrap();

        assert_eq!(rendered.text, "Primary alice -> bob @ 2");
    }

    fn sms_template(provider_templates: BTreeMap<String, ProviderTemplate>) -> NotifyTemplate {
        NotifyTemplate {
            id: Id::from_string("template-sms"),
            organization_id: Id::new(),
            category: NotifyCategory::Alert,
            channel: Some(NotifyChannel::Sms),
            body: "[{{severity}}] {{rule.name}}".into(),
            format: "text".into(),
            provider_templates,
            card: None,
        }
    }

    fn alert_event() -> NotifyEvent {
        NotifyEvent {
            id: "alert.triggered:1".into(),
            event_type: "alert.triggered".into(),
            organization_id: Id::new(),
            occurred_at: TimestampMicros(1),
            attributes: serde_json::json!({
                "severity": "critical",
                "rule_name": "Disk full",
                "labels": {"host": "db-1"}
            }),
        }
    }

    #[test]
    fn an_sms_template_is_the_whole_text_and_fills_the_provider_variables() {
        let approved = ProviderTemplate {
            code: " SMS_1 ".into(),
            params: BTreeMap::from([
                ("name".to_string(), "{{rule.name}}".to_string()),
                ("host".to_string(), "on {{labels.host}}".to_string()),
            ]),
            max_param_chars: Some(20),
        };

        let rendered = render_message(
            &sms_template(BTreeMap::from([("aliyun".to_string(), approved)])),
            &alert_event(),
            &message(),
        )
        .unwrap();

        assert_eq!(rendered.title, "");
        assert_eq!(rendered.text, "[critical] Disk full");

        let aliyun = &rendered.provider_templates["aliyun"];
        assert_eq!(aliyun.code, "SMS_1");
        assert_eq!(aliyun.params["name"], "Disk full");
        assert_eq!(aliyun.params["host"], "on db-1");
        assert_eq!(aliyun.max_param_chars, Some(20));
    }

    #[test]
    fn each_channel_gets_its_own_template_or_the_general_one() {
        let general = NotifyTemplate {
            id: Id::from_string("template-general"),
            channel: None,
            body: "**{{rule.name}}**".into(),
            format: "markdown".into(),
            ..sms_template(BTreeMap::new())
        };

        let messages = PolicyMessages {
            message: message(),
            general: Some(general),
            channels: BTreeMap::from([(NotifyChannel::Sms, sms_template(BTreeMap::new()))]),
        };

        let sms = messages
            .for_channel(NotifyChannel::Sms, &alert_event())
            .unwrap();
        assert_eq!(sms.text, "[critical] Disk full");

        let im = messages
            .for_channel(NotifyChannel::Im, &alert_event())
            .unwrap();
        assert_eq!(im.title, "Title");
        assert_eq!(im.markdown.as_deref(), Some("**Disk full**"));

        let untouched = PolicyMessages {
            message: message(),
            general: None,
            channels: BTreeMap::new(),
        };
        assert_eq!(
            untouched
                .for_channel(NotifyChannel::Email, &alert_event())
                .unwrap()
                .text,
            "Text"
        );
    }

    #[test]
    fn only_plain_text_templates_carry_provider_templates() {
        let approved = |code: &str, name: &str, value: &str| {
            BTreeMap::from([(
                "aliyun".to_string(),
                ProviderTemplate {
                    code: code.into(),
                    params: BTreeMap::from([(name.to_string(), value.to_string())]),
                    max_param_chars: None,
                },
            )])
        };

        let sms = Some(NotifyChannel::Sms);
        let valid = approved("SMS_1", "name", "{{rule.name}}");

        assert!(validate_notify_template(sms, "text", "body", &valid, None).is_ok());
        assert!(validate_notify_template(sms, "markdown", "body", &BTreeMap::new(), None).is_err());
        assert!(validate_notify_template(None, "text", "body", &valid, None).is_err());

        assert!(
            validate_notify_template(Some(NotifyChannel::Im), "text", "body", &valid, None)
                .is_err()
        );

        for broken in [
            approved("SMS 1", "name", "x"),
            approved("SMS_1", "${name}", "x"),
            approved("SMS_1", "name", " "),
        ] {
            assert!(validate_notify_template(sms, "text", "body", &broken, None).is_err());
        }

        assert!(validate_notify_template(None, "pdf", "body", &BTreeMap::new(), None).is_err());
    }

    #[test]
    fn a_test_send_fills_the_template_with_the_sample_event() {
        let rendered = render_sample(&sms_template(BTreeMap::new()), &Id::new()).unwrap();
        assert_eq!(rendered.text, "[critical] API unavailable");
    }

    fn im_template(card: NotifyCard) -> NotifyTemplate {
        NotifyTemplate {
            id: Id::from_string("template-im"),
            channel: Some(NotifyChannel::Im),
            body: "**{{rule.name}}** is {{incident.status}}".into(),
            format: "markdown".into(),
            card: Some(card),
            ..sms_template(BTreeMap::new())
        }
    }

    fn field(label: &str, value: &str) -> NotifyCardField {
        NotifyCardField {
            label: label.into(),
            value: value.into(),
        }
    }

    fn link(label: &str, url: &str) -> NotifyCardLink {
        NotifyCardLink {
            label: label.into(),
            url: url.into(),
        }
    }

    #[test]
    fn a_card_shows_only_the_fields_and_links_its_event_fills() {
        let card = NotifyCard {
            title: "[{{severity}}] {{rule.name}}".into(),
            color: NotifyCardColor::Severity,
            fields: vec![
                field("Host", "{{labels.host}}"),
                field("Service", "{{labels.service}}"),
                field("Owner", "  "),
            ],
            links: vec![
                link("Runbook", "{{annotations.runbook_url}}"),
                link("Dashboard", "https://grafana.example.com/d/{{labels.host}}"),
                link("Mail", "mailto:{{labels.host}}"),
            ],
            note: "Sent by MoleSignal".into(),
        };

        let rendered = render_message(&im_template(card), &alert_event(), &message()).unwrap();
        let card = rendered.card.unwrap();

        assert_eq!(rendered.title, "Title");
        assert_eq!(
            rendered.markdown.as_deref(),
            Some("**Disk full** is {{incident.status}}")
        );
        assert_eq!(card.title, "[critical] Disk full");
        assert_eq!(card.fields, vec![field("Host", "db-1")]);
        assert_eq!(
            card.links,
            vec![link("Dashboard", "https://grafana.example.com/d/db-1")]
        );
        assert_eq!(card.note, "Sent by MoleSignal");
    }

    #[test]
    fn only_instant_messaging_templates_carry_a_card() {
        let card = NotifyCard {
            fields: vec![field("Service", "{{labels.service}}")],
            links: vec![link("Runbook", "{{annotations.runbook_url}}")],
            ..NotifyCard::default()
        };
        let im = Some(NotifyChannel::Im);
        let none = BTreeMap::new();

        assert!(validate_notify_template(im, "markdown", "body", &none, Some(&card)).is_ok());
        assert!(validate_notify_template(None, "markdown", "body", &none, Some(&card)).is_err());

        for broken in [
            NotifyCard {
                fields: vec![field("", "x")],
                ..card.clone()
            },
            NotifyCard {
                links: vec![link("Mail", "mailto:ops@example.com")],
                ..card.clone()
            },
            NotifyCard {
                fields: vec![field("Service", "x"); 11],
                ..card.clone()
            },
        ] {
            assert!(
                validate_notify_template(im, "markdown", "body", &none, Some(&broken)).is_err()
            );
        }
    }

    #[test]
    fn readable_values_are_written_for_people() {
        let event = NotifyEvent {
            id: "alert.resolved:1".into(),
            event_type: "alert.resolved".into(),
            organization_id: Id::new(),
            occurred_at: TimestampMicros(1_785_290_400_000_000),
            attributes: serde_json::json!({
                "started_at": 1_785_282_900_000_000_i64,
                "resolved_at": 1_785_286_800_000_000_i64,
                "threshold": 500.0,
                "ratio": 0.25,
                "affected_services": ["api", "checkout"],
                "labels": {"env": "production", "service": "api"},
                "step_index": 1,
                "loop_index": 0
            }),
        };

        let values = template_values(&event, &message());
        let value = |key: &str| values.get(key).map(String::as_str);

        assert_eq!(value("event.time"), Some("2026-07-29 02:00:00 UTC"));
        assert_eq!(
            value("incident.started_time"),
            Some("2026-07-28 23:55:00 UTC")
        );
        assert_eq!(
            value("incident.resolved_time"),
            Some("2026-07-29 01:00:00 UTC")
        );
        assert_eq!(value("incident.duration"), Some("1h 5m"));
        assert_eq!(value("threshold"), Some("500"));
        assert_eq!(value("ratio"), Some("0.25"));
        assert_eq!(value("affected_services"), Some("api, checkout"));
        assert_eq!(value("labels"), Some("env=production, service=api"));
        assert_eq!(value("escalation.step"), Some("2"));
        assert_eq!(value("escalation.loop"), Some("1"));
        assert_eq!(value("incident.acknowledged_time"), None);
    }

    /// 预设照着写的那种事件：示例事件换成确认或恢复了的告警。
    fn preset_sample(preset: &NotifyTemplatePreset) -> (NotifyEvent, NotifyMessage) {
        let (mut event, message) = sample(&Id::new(), preset.category);
        let attributes = &mut event.attributes;

        if matches!(
            preset.event_type.as_str(),
            "alert.acknowledged" | "alert.resolved"
        ) {
            attributes["status"] = serde_json::json!("acknowledged");
            attributes["acknowledged_by"] = serde_json::json!("alice");
            attributes["acknowledged_at"] = serde_json::json!(1_785_283_800_000_000_i64);
        }

        if preset.event_type == "alert.resolved" {
            attributes["status"] = serde_json::json!("resolved");
            attributes["resolved_at"] = serde_json::json!(1_785_286_800_000_000_i64);
        }

        event.event_type = preset.event_type.clone();
        (event, message)
    }

    #[test]
    fn every_preset_is_written_in_both_languages_and_fills_completely() {
        let english = notify_template_preset_catalog(&seeded_pack(
            NOTIFY_TEMPLATE_PRESET_LANGUAGE_PREFIX,
            "en-us",
        ));
        let chinese = notify_template_preset_catalog(&seeded_pack(
            NOTIFY_TEMPLATE_PRESET_LANGUAGE_PREFIX,
            "zh-cn",
        ));
        let han = |text: &str| text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));

        assert_eq!(english.len(), notify_template_presets().len());
        assert_eq!(chinese.len(), english.len());

        for (english, chinese) in english.iter().zip(&chinese) {
            assert!(!han(&english.body), "{} is English", english.key);
            assert!(han(&chinese.body), "{} has a Chinese body", chinese.key);

            for preset in [english, chinese] {
                let (event, message) = preset_sample(preset);
                let filled = fill(&preset.body, &template_values(&event, &message));

                assert!(
                    !filled.contains("{{"),
                    "{} leaves a placeholder: {filled}",
                    preset.key
                );
            }
        }
    }

    #[test]
    fn a_duration_is_its_two_largest_units() {
        let seconds = |secs: i64| duration_text(secs * 1_000_000);

        assert_eq!(duration_text(400_000), "0s");
        assert_eq!(seconds(59), "59s");
        assert_eq!(seconds(260), "4m 20s");
        assert_eq!(seconds(3_600), "1h");
        assert_eq!(seconds(3_900), "1h 5m");
        assert_eq!(seconds(90_061), "1d 1h");
        assert_eq!(seconds(-5), "0s");
    }

    fn webhook_template(body: &str, format: &str) -> NotifyTemplate {
        NotifyTemplate {
            id: Id::from_string("template-hook"),
            organization_id: Id::new(),
            category: NotifyCategory::Alert,
            channel: Some(NotifyChannel::Webhook),
            body: body.into(),
            format: format.into(),
            provider_templates: BTreeMap::new(),
            card: None,
        }
    }

    #[test]
    fn a_json_template_escapes_its_values_and_says_it_is_json() {
        let mut event = alert_event();
        event.attributes["rule_name"] = json!("Disk \"full\"\non db-1");

        let template = webhook_template(
            r#"{"title": "{{rule.name}}", "level": "{{severity}}"}"#,
            "json",
        );

        let rendered = render_message(&template, &event, &message()).unwrap();
        let body: Value = serde_json::from_str(&rendered.text).unwrap();

        assert_eq!(body["title"], "Disk \"full\"\non db-1");
        assert_eq!(body["level"], "critical");
        assert_eq!(rendered.metadata["body_format"], "json");
        assert!(rendered.markdown.is_none());
    }

    #[test]
    fn json_templates_are_for_webhooks_and_must_be_json() {
        let check = |channel, body: &str| {
            validate_notify_template(channel, "json", body, &BTreeMap::new(), None)
        };

        assert!(
            check(
                Some(NotifyChannel::Webhook),
                r#"{"value": {{value}}, "t": "{{x}}"}"#
            )
            .is_ok()
        );
        assert!(check(Some(NotifyChannel::Webhook), r#"{"value": "#).is_err());
        assert!(check(Some(NotifyChannel::Im), r#"{"a": 1}"#).is_err());
        assert!(check(None, r#"{"a": 1}"#).is_err());
    }

    #[test]
    fn a_condition_chooses_the_words_by_the_event() {
        let template = NotifyTemplate {
            body: r#"{{#if severity == "critical"}}马上处理{{else}}有空看看{{/if}}：{{rule.name}}"#
                .into(),
            ..sms_template(BTreeMap::new())
        };

        let rendered = render_message(&template, &alert_event(), &message()).unwrap();
        assert_eq!(rendered.text, "马上处理：Disk full");

        let mut calm = alert_event();
        calm.attributes["severity"] = json!("info");
        let rendered = render_message(&template, &calm, &message()).unwrap();
        assert_eq!(rendered.text, "有空看看：Disk full");

        assert!(
            validate_notify_template(
                Some(NotifyChannel::Sms),
                "text",
                "{{#if severity}}unclosed",
                &BTreeMap::new(),
                None,
            )
            .is_err()
        );
    }
}
