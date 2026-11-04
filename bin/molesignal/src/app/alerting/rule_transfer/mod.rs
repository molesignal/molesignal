// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 规则的导入导出：导出成按名称引用的包（[`bundle`]），导入这样的包，或 Prometheus 的告警规则文件
//! （[`prometheus`]）。
//!
//! 导入先预演：每一条按名称对上已有的规则——没有就新建，正好一条就比较字段（不变或更新），同名
//! 的有几条就报冲突；也可以只新建（已有的跳过）。整包都说得通才写，一个事务写进去。

use serde::Serialize;

use crate::shared::{Error, Result, ids::Id};

pub mod bundle;
pub mod prometheus;

use bundle::{BUNDLE_FORMAT, PortableRule, RuleBundle};

/// 一次最多导入几条。
pub const MAX_IMPORT_RULES: usize = 500;

/// 导入的内容最长多少字节。
pub const MAX_IMPORT_BYTES: usize = 4 * 1024 * 1024;

/// 读出来的是哪种文件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportFormat {
    Molesignal,
    Prometheus,
}

/// 导进来的一条规则会怎样。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportAction {
    Create,
    Update,
    Unchanged,
    /// 只新建时，已经有的同名规则不动。
    Skip,
    Error,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportItem {
    pub name: String,
    pub action: ImportAction,
    /// 更新时改了哪些字段。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 新建或更新的那条规则。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<Id>,
}

impl ImportItem {
    pub fn failed(name: &str, error: impl Into<String>) -> Self {
        Self {
            name: name.to_string(),
            action: ImportAction::Error,
            changes: Vec::new(),
            error: Some(error.into()),
            rule_id: None,
        }
    }
}

/// 预演或导入的结果。
#[derive(Debug, Clone, Serialize)]
pub struct ImportReport {
    pub format: ImportFormat,
    pub items: Vec<ImportItem>,
    /// 读文件时略过的东西：记录规则、不支持的字段。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// 写进去了没有：预演，或有一条出错时都没写。
    pub applied: bool,
}

/// 读出要导入的规则：导出包（JSON 或 YAML，带 `format`），或 Prometheus 的规则文件。
pub fn parse(content: &str) -> Result<(ImportFormat, Vec<PortableRule>, Vec<String>)> {
    if content.len() > MAX_IMPORT_BYTES {
        return Err(Error::invalid(format!(
            "an import is at most {} MiB",
            MAX_IMPORT_BYTES / 1024 / 1024
        )));
    }

    let value: serde_yaml::Value = serde_yaml::from_str(content).map_err(|error| {
        Error::invalid(format!("the content is neither JSON nor YAML: {error}"))
    })?;

    let (format, rules, notes) = if value.get("format").is_some() {
        let bundle: RuleBundle = serde_yaml::from_value(value)
            .map_err(|error| Error::invalid(format!("not a rule bundle: {error}")))?;

        if bundle.format != BUNDLE_FORMAT {
            return Err(Error::invalid(format!(
                "unknown bundle format `{}`; expected `{BUNDLE_FORMAT}`",
                bundle.format
            )));
        }

        (ImportFormat::Molesignal, bundle.rules, Vec::new())
    } else if prometheus::looks_like(content) {
        let (rules, notes) = prometheus::parse(content)?;
        (ImportFormat::Prometheus, rules, notes)
    } else {
        return Err(Error::invalid(
            "expected a MoleSignal rule bundle or a Prometheus rule file (`groups:`)",
        ));
    };

    if rules.len() > MAX_IMPORT_RULES {
        return Err(Error::invalid(format!(
            "an import has at most {MAX_IMPORT_RULES} rules"
        )));
    }

    Ok((format, rules, notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_and_a_prometheus_file_are_told_apart() {
        let bundle = r#"{"format": "molesignal.alert-rules/v1", "rules": []}"#;
        assert_eq!(parse(bundle).unwrap().0, ImportFormat::Molesignal);

        let prometheus =
            "groups:\n  - name: g\n    rules:\n      - alert: Down\n        expr: up == 0\n";
        let (format, rules, _) = parse(prometheus).unwrap();
        assert_eq!(format, ImportFormat::Prometheus);
        assert_eq!(rules.len(), 1);

        assert!(parse(r#"{"format": "other/v9", "rules": []}"#).is_err());
        assert!(parse("just: text").is_err());
        assert!(parse("{ not yaml").is_err());
    }
}
