// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 修复动作的模板：把 `{{incident.*}}` 和 `{{params.*}}` 换成事件和发起时填的值。填进地址的值做
//! 百分号编码，填进请求体（JSON）的值按 JSON 字符串转义。

use std::collections::BTreeMap;

use super::incident::Incident;

/// 填模板时怎样转义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escape {
    None,
    /// 填进地址：百分号编码。
    Url,
    /// 填进 JSON 字符串：按 JSON 转义，不带两边的引号。
    Json,
}

/// 把 `{{name}}` 换成 `vars` 里的值；没有的名字换成空串。
pub fn render_template(template: &str, vars: &BTreeMap<String, String>, escape: Escape) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);

        let Some(end) = rest[start..].find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };

        let name = rest[start + 2..start + end].trim();
        let value = vars.get(name).map(String::as_str).unwrap_or_default();

        match escape {
            Escape::None => out.push_str(value),
            Escape::Url => out.push_str(&percent_encode(value)),
            Escape::Json => {
                let quoted = serde_json::to_string(value).unwrap_or_default();
                out.push_str(&quoted[1..quoted.len().saturating_sub(1)]);
            }
        }

        rest = &rest[start + end + 2..];
    }

    out.push_str(rest);
    out
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());

    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }

    out
}

/// 请求体模板填上值以后是不是 JSON：每个 `{{name}}` 先当作 `0`，所以引号里的占位符和直接当数字用的
/// 占位符（如 `{"replicas": {{params.replicas}}}`）都算数。
pub fn reads_as_json(template: &str) -> bool {
    let mut zeroed = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        let Some(end) = rest[start..].find("}}") else {
            break;
        };

        zeroed.push_str(&rest[..start]);
        zeroed.push('0');
        rest = &rest[start + end + 2..];
    }

    zeroed.push_str(rest);

    serde_json::from_str::<serde_json::Value>(&zeroed).is_ok()
}

/// 一个事件和填好的参数给模板用的变量。
pub fn template_vars(
    incident: &Incident,
    incident_url: &str,
    inputs: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();

    vars.insert("incident.id".into(), incident.id.to_string());
    vars.insert("incident.summary".into(), incident.summary.clone());
    vars.insert(
        "incident.severity".into(),
        incident.severity.as_str().into(),
    );
    vars.insert("incident.status".into(), incident.status.as_str().into());
    vars.insert("incident.url".into(), incident_url.into());
    vars.insert(
        "incident.service".into(),
        incident
            .labels
            .get("service")
            .cloned()
            .or_else(|| incident.affected_services.first().cloned())
            .unwrap_or_default(),
    );

    for (key, value) in inputs {
        vars.insert(format!("params.{key}"), value.clone());
    }

    vars
}
