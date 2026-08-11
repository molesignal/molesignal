// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashSet, sync::LazyLock};

use serde::Deserialize;

use crate::shared::{Error, Result};

const MANIFEST_JSON: &str = include_str!("manifest.json");
pub const INSTRUCTIONS: &str = include_str!("instructions.md");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DashboardAuthoringManifest {
    pub id: String,
    pub version: u32,
    pub purpose: String,
    pub instruction_template_key: String,
    pub authoring_contract_versions: Vec<u32>,
    pub required_tools: Vec<String>,
    pub optional_tools: Vec<String>,
    pub trigger_summaries: Vec<String>,
    pub negative_examples: Vec<String>,
    pub max_repair_attempts: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationSource {
    ExplicitCapability,
    AnalysisMode,
    FreeText,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Activation {
    pub source: ActivationSource,
    pub input_complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Compatibility {
    pub preview_only: bool,
}

static MANIFEST: LazyLock<DashboardAuthoringManifest> = LazyLock::new(|| {
    let manifest: DashboardAuthoringManifest =
        serde_json::from_str(MANIFEST_JSON).expect("Dashboard authoring manifest must be valid");
    assert_eq!(manifest.id, "dashboard-authoring");
    assert_eq!(manifest.purpose, "dashboard_authoring");
    assert!(!manifest.authoring_contract_versions.is_empty());
    assert!(manifest.max_repair_attempts <= 2);
    manifest
});

#[must_use]
pub fn manifest() -> &'static DashboardAuthoringManifest {
    &MANIFEST
}

pub fn resolve(
    explicit_capability: Option<&str>,
    request_analysis_mode: Option<&str>,
    chat_analysis_mode: Option<&str>,
    content: &str,
    has_time_range: bool,
) -> Result<Option<Activation>> {
    if let Some(capability) = explicit_capability {
        if capability != "dashboard_authoring" {
            return Err(Error::invalid(format!(
                "unknown Mole Agent capability `{capability}`"
            )));
        }
        return Ok(Some(Activation {
            source: ActivationSource::ExplicitCapability,
            input_complete: input_complete(content, has_time_range),
        }));
    }
    if request_analysis_mode == Some("dashboard") || chat_analysis_mode == Some("dashboard") {
        return Ok(Some(Activation {
            source: ActivationSource::AnalysisMode,
            input_complete: input_complete(content, has_time_range),
        }));
    }
    if high_confidence_intent(content) {
        return Ok(Some(Activation {
            source: ActivationSource::FreeText,
            input_complete: input_complete(content, has_time_range),
        }));
    }
    Ok(None)
}

pub fn validate_compatibility(
    enabled_tools: impl IntoIterator<Item = String>,
    compiler_versions: &[u32],
) -> Result<Compatibility> {
    let enabled = enabled_tools.into_iter().collect::<HashSet<_>>();
    let manifest = manifest();
    let missing = manifest
        .required_tools
        .iter()
        .filter(|tool| !enabled.contains(tool.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(Error::forbidden(format!(
            "Dashboard authoring is unavailable because required tools are disabled: {}",
            missing.join(", ")
        )));
    }
    if !manifest
        .authoring_contract_versions
        .iter()
        .any(|version| compiler_versions.contains(version))
    {
        return Err(Error::conflict(
            "Dashboard skill and compiler authoring contract versions do not overlap",
        ));
    }
    let preview_only = manifest
        .optional_tools
        .iter()
        .any(|tool| !enabled.contains(tool.as_str()));
    Ok(Compatibility { preview_only })
}

fn high_confidence_intent(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    let has_dashboard = lower.contains("dashboard") || content.contains("仪表盘");
    let has_creation = ["create", "build", "generate", "make"]
        .iter()
        .any(|word| lower.contains(word))
        || ["创建", "生成", "搭建", "制作"]
            .iter()
            .any(|word| content.contains(word));
    has_dashboard && has_creation
}

fn input_complete(content: &str, has_time_range: bool) -> bool {
    let lower = content.to_ascii_lowercase();
    let has_data_topic = [
        "log", "metric", "trace", "profile", "latency", "error", "traffic", "cpu", "memory",
        "service", "request",
    ]
    .iter()
    .any(|word| lower.contains(word))
        || [
            "日志", "指标", "链路", "剖析", "延迟", "错误", "流量", "服务", "请求",
        ]
        .iter()
        .any(|word| content.contains(word));
    let has_time = has_time_range
        || ["last ", "today", "yesterday", "hour", "day", "week"]
            .iter()
            .any(|word| lower.contains(word))
        || ["最近", "过去", "今天", "昨天", "小时", "天", "周"]
            .iter()
            .any(|word| content.contains(word));
    has_data_topic && has_time
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_text_routing_is_conservative() {
        assert!(
            resolve(None, None, None, "创建最近一小时服务错误率仪表盘", false)
                .unwrap()
                .is_some()
        );
        assert!(
            resolve(None, None, None, "解释当前 dashboard 为什么慢", false)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn explicit_starter_and_analysis_mode_take_precedence() {
        let explicit = resolve(Some("dashboard_authoring"), None, None, "服务错误率", true)
            .unwrap()
            .unwrap();
        assert_eq!(explicit.source, ActivationSource::ExplicitCapability);
        assert!(explicit.input_complete);

        let mode = resolve(None, Some("dashboard"), None, "需要一个仪表盘", false)
            .unwrap()
            .unwrap();
        assert_eq!(mode.source, ActivationSource::AnalysisMode);
        assert!(!mode.input_complete);
        assert!(resolve(Some("unknown"), Some("dashboard"), None, "", false).is_err());
    }

    #[test]
    fn compatibility_degrades_to_preview_only_or_fails_closed() {
        let required_only = vec![
            "get_dashboard_capabilities".to_string(),
            "prepare_dashboard".to_string(),
        ];
        assert!(
            validate_compatibility(required_only.clone(), &[1])
                .unwrap()
                .preview_only
        );
        let mut complete = required_only;
        complete.push("propose_dashboard_creation".into());
        assert!(
            !validate_compatibility(complete.clone(), &[1])
                .unwrap()
                .preview_only
        );
        assert!(validate_compatibility(complete, &[2]).is_err());
        assert!(validate_compatibility(vec!["prepare_dashboard".into()], &[1]).is_err());
    }

    #[test]
    fn ordinary_chat_does_not_activate_dashboard_authoring() {
        assert!(
            resolve(
                None,
                None,
                None,
                "查询最近一小时 checkout 服务的错误日志",
                true,
            )
            .unwrap()
            .is_none()
        );
        assert!(
            resolve(None, None, None, "解释现有仪表盘的延迟曲线", true)
                .unwrap()
                .is_none()
        );
    }
}
