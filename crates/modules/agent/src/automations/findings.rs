// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 发现触发：自动发现新找到的一个问题是否满足规则的筛选。同一条发现只调查一次。

use serde::{Deserialize, Serialize};

use crate::domain::alerting::{discovery::Finding, incident::Severity};

/// 哪些新发现触发：每一项留空表示不限，填了的都要满足。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingFilter {
    /// 发现的种类，如 `apm_anomaly`、`log_error_surge`。
    #[serde(default)]
    pub kinds: Vec<String>,
    /// 至少这么严重：`info`、`warning`、`error`、`critical`。
    #[serde(default)]
    pub min_severity: Option<String>,
    /// 对象：APM 服务、RUM 应用、日志里的服务、主机或容器。
    #[serde(default)]
    pub subjects: Vec<String>,
}

impl FindingFilter {
    pub fn matches(&self, finding: &Finding) -> bool {
        let kind =
            self.kinds.is_empty() || self.kinds.iter().any(|kind| kind == finding.kind.as_str());
        let severity = self
            .min_severity
            .as_deref()
            .and_then(Severity::from_label)
            .is_none_or(|bar| finding.severity >= bar);
        let subject = self.subjects.is_empty()
            || self.subjects.iter().any(|subject| {
                *subject == finding.subject
                    || finding
                        .subject
                        .rsplit_once('/')
                        .is_some_and(|(_, name)| name == subject)
            });

        kind && severity && subject
    }
}
