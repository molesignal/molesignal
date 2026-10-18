// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 消息里的引用：输入框里用 `@` 插入的事件、告警规则、服务、仪表盘与数据流。
//!
//! 发消息时后端核对提问人能不能读每个引用，再把它们解析成上下文块交给模型（事件的摘要、触发
//! 查询、时间窗、相关服务……）；用户消息上记下引用，界面据此画胶囊。

use serde::{Deserialize, Serialize};

/// 一条消息最多带这么多引用。
pub const MAX_REFERENCES: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    Incident,
    AlertRule,
    Service,
    Dashboard,
    Stream,
}

impl ReferenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Incident => "incident",
            Self::AlertRule => "alert_rule",
            Self::Service => "service",
            Self::Dashboard => "dashboard",
            Self::Stream => "stream",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageReference {
    pub kind: ReferenceKind,
    pub id: String,
    /// 胶囊上显示的名字；后端解析时按当前的名字改写。
    #[serde(default)]
    pub label: String,
}

/// 一个引用交给模型时的样子。
pub fn context_block(reference: &MessageReference, facts: &str) -> String {
    format!(
        "<reference kind=\"{}\" id=\"{}\" name=\"{}\">\n{}\n</reference>",
        reference.kind.as_str(),
        reference.id.replace('"', "'"),
        reference.label.replace('"', "'"),
        facts.trim()
    )
}

/// 引用的说明放在这些块前面。
pub const REFERENCES_INTRODUCTION: &str = "The person attached the objects below to this question. Use them as the subject of the question; fetch more about them with your tools when needed.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reference_names_what_it_is() {
        let reference: MessageReference = serde_json::from_value(
            serde_json::json!({"kind": "alert_rule", "id": "r1", "label": "API \"5xx\""}),
        )
        .unwrap();

        let block = context_block(&reference, "query: count(5xx) > 10\n");

        assert!(block.starts_with("<reference kind=\"alert_rule\" id=\"r1\" name=\"API '5xx'\">"));
        assert!(block.ends_with("count(5xx) > 10\n</reference>"));
    }
}
