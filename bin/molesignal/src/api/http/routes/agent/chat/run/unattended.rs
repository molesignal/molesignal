// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 没有人可问的一轮：自动化与事件触发的 Run。
//!
//! 提问、确认与「授权以继续」都不等：记下被挡住的地方（运行记录里的原因），交给模型的结果照
//! 没有人可问处理。任务清单与提醒照常写进会话，点开运行记录能看到全过程。

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use super::{
    super::super::tool_dispatcher::{Reply, RunInteraction},
    RunControl,
};
use crate::{
    agent::{
        automations::reasons,
        model::RiskLevel,
        runs::{QuestionKind, Run, RunEventKind, RunSource},
    },
    app::mcp::CredentialNeed,
};

/// 这一轮没有人在等着回答。
pub(super) fn is_unattended(run: &Run) -> bool {
    run.source != RunSource::Chat
}

pub(super) struct Unattended {
    control: Arc<RunControl>,
}

impl Unattended {
    pub(super) fn new(control: Arc<RunControl>) -> Self {
        Self { control }
    }
}

#[async_trait]
impl RunInteraction for Unattended {
    async fn todo(&self, items: Value) {
        self.control
            .emit(RunEventKind::Todo, json!({ "items": items }))
            .await;
    }

    async fn ask(&self, _question: String, _kind: QuestionKind, _options: Vec<String>) -> Reply {
        self.control.block(reasons::QUESTION);
        Reply::Unattended
    }

    async fn confirm(&self, _tool: &str, _arguments: &Value, _risk: RiskLevel) -> Reply {
        self.control.block(reasons::CONFIRMATION);
        Reply::Unattended
    }

    async fn authorize(&self, _tool: &str, _need: &CredentialNeed) -> Reply {
        self.control.block(reasons::CREDENTIAL);
        Reply::Unattended
    }

    async fn notice(&self, payload: Value) {
        self.control.emit(RunEventKind::Notice, payload).await;
    }
}
