// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一次巡检向模型要什么。调模型、调工具要用到模型配置和工具运行时，那是 infra 的事，
//! 这一层只说清楚端口；实现在 `bootstrap::workers::log_inspection`。

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::{
    domain::{iam::IamContext, log_inspection::InspectionToolCall},
    shared::ids::Id,
};

/// 巡检过程中调用过的工具，按调用顺序。
///
/// 记在执行的那个 future 之外：巡检超时被丢弃后，已经发生的调用仍然留得下来。
#[derive(Debug, Default)]
pub struct ToolCallLog(Mutex<Vec<InspectionToolCall>>);

impl ToolCallLog {
    pub fn record(&self, name: &str, ok: bool) {
        self.0.lock().push(InspectionToolCall {
            name: name.to_string(),
            ok,
        });
    }

    pub fn calls(&self) -> Vec<InspectionToolCall> {
        self.0.lock().clone()
    }
}

pub struct AgentRequest {
    /// 以谁的权限读日志。
    pub identity: IamContext,
    pub provider_id: Id,
    /// `None` 时用模型配置的默认模型。
    pub model: Option<String>,
    /// 这次巡检只读这一个数据流。
    pub stream: String,
    /// 系统指令，按顺序给模型。
    pub instructions: Vec<String>,
    /// 巡检任务本身。
    pub task: String,
    pub tool_calls: Arc<ToolCallLog>,
}

#[derive(Debug, Clone)]
pub struct AgentAnswer {
    /// 模型最后写下的内容：巡检报告。
    pub content: String,
    /// 实际用的模型名。
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
}

/// 模型没能给出报告。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentFailure {
    /// 模型配置用不了：不存在、已停用、没有密钥。
    Model(String),
    /// 调用模型的过程失败了。
    Call(String),
    /// 组织今天的 token 预算用完了，没有调用模型。
    Budget(String),
}

/// 让模型巡检一次。
#[async_trait]
pub trait InspectionAgent: Send + Sync {
    /// 这个模型配置现在能不能用：在这个组织里、已启用、有密钥。保存巡检时先问一遍，
    /// 不让一条注定每次都失败的巡检存进去。
    async fn check_model(&self, org_id: &Id, provider_id: &Id) -> Result<(), AgentFailure>;

    async fn inspect(&self, request: AgentRequest) -> Result<AgentAnswer, AgentFailure>;
}
