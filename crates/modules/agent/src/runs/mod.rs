// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一轮回答 = 一个 Run。
//!
//! 发消息只把 Run 排进队列；后台在某个节点上执行它，过程写成按序号递增的事件（[`RunEvent`]），
//! 浏览器从任意节点按序号续读，切走页面或刷新都不打断回答。停止、提问人的回答与确认都记在
//! Run 上，由执行它的节点取走。同一个会话的 Run 按先后一个接一个执行。
//!
//! - 执行身份记成 [`RunPrincipal`]，执行时据此重新核对账号与权限。
//! - 执行中的 Run 持有租约，节点隔一会儿续一次；租约过期说明节点没了，Run 标为
//!   [`RunStatus::Interrupted`]。

mod context;
mod events;
mod signals;
mod transcript;

use std::time::Duration;

use async_trait::async_trait;
pub use context::{
    COMPACTION_INSTRUCTION, COMPACTION_KEEP_TURNS, compaction_due, compaction_split,
    estimate_tokens, language_instruction, summary_text,
};
use domain::iam::{IamContext, IamScope};
pub use events::{
    NewRunEvent, RunEvent, RunEventKind, TEXT_FLUSH_BYTES, TEXT_FLUSH_MILLIS, TextBuffer,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use signals::{RunSignalHub, RunSignals};
pub use transcript::{
    MAX_INLINE_RESULT_BYTES, TranscriptBuilder, TranscriptCall, TranscriptStep, inline_result,
    transcript_messages,
};

use crate::{
    agent::{model::RiskLevel, usage::Cost},
    shared::{Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    /// 等提问人回答一个问题。
    WaitingInput,
    /// 等提问人确认执行一个工具。
    WaitingApproval,
    Succeeded,
    Failed,
    Cancelled,
    /// 执行它的节点没了。
    Interrupted,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::WaitingInput => "waiting_input",
            Self::WaitingApproval => "waiting_approval",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Queued,
            Self::Running,
            Self::WaitingInput,
            Self::WaitingApproval,
            Self::Succeeded,
            Self::Failed,
            Self::Cancelled,
            Self::Interrupted,
        ]
        .into_iter()
        .find(|status| status.as_str() == value)
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }

    /// 正在某个节点上执行（含等待回答与确认）。
    pub fn is_live(self) -> bool {
        matches!(
            self,
            Self::Running | Self::WaitingInput | Self::WaitingApproval
        )
    }
}

/// Run 从哪来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSource {
    Chat,
    Automation,
    Incident,
    Api,
    Im,
}

impl RunSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Automation => "automation",
            Self::Incident => "incident",
            Self::Api => "api",
            Self::Im => "im",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Chat,
            Self::Automation,
            Self::Incident,
            Self::Api,
            Self::Im,
        ]
        .into_iter()
        .find(|source| source.as_str() == value)
    }
}

/// 以谁的身份执行。只记身份，不记权限：执行时重新解析，这期间被收回的权限不会被用上。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunPrincipal {
    pub user_id: Id,
    pub scope: IamScope,
    #[serde(default)]
    pub credential_role_id: Option<Id>,
    #[serde(default)]
    pub credential_application_id: Option<String>,
    #[serde(default)]
    pub credential_service_account_id: Option<Id>,
}

impl RunPrincipal {
    pub fn of(ctx: &IamContext) -> Self {
        Self {
            user_id: ctx.user_id.clone(),
            scope: ctx.scope,
            credential_role_id: ctx.credential_role_id.clone(),
            credential_application_id: ctx.credential_application_id.clone(),
            credential_service_account_id: ctx.credential_service_account_id.clone(),
        }
    }

    /// 还没解析权限的身份，交给 IAM 补全。
    pub fn unresolved_context(&self, org_id: &Id) -> IamContext {
        IamContext {
            user_id: self.user_id.clone(),
            org_id: org_id.clone(),
            display_role: String::new(),
            roles: Vec::new(),
            credential_role_id: self.credential_role_id.clone(),
            credential_application_id: self.credential_application_id.clone(),
            credential_service_account_id: self.credential_service_account_id.clone(),
            scope: self.scope,
            permissions: Default::default(),
            policy_version: 0,
        }
    }
}

/// 提问的方式。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionKind {
    Single,
    Multiple,
    #[default]
    Text,
}

/// 在等提问人的东西。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Pending {
    Question {
        call_id: String,
        question: String,
        #[serde(default)]
        kind: QuestionKind,
        #[serde(default)]
        options: Vec<String>,
    },
    Confirmation {
        call_id: String,
        tool: String,
        arguments: Value,
        risk: RiskLevel,
    },
}

impl Pending {
    pub fn call_id(&self) -> &str {
        match self {
            Self::Question { call_id, .. } | Self::Confirmation { call_id, .. } => call_id,
        }
    }

    pub fn status(&self) -> RunStatus {
        match self {
            Self::Question { .. } => RunStatus::WaitingInput,
            Self::Confirmation { .. } => RunStatus::WaitingApproval,
        }
    }
}

/// 提问人的回答或决定。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunResponse {
    Answer {
        call_id: String,
        /// 选中的选项。
        #[serde(default)]
        choices: Vec<String>,
        /// 写下的回答。
        #[serde(default)]
        text: String,
        /// 谁回答的：团队会话里不一定是发起这一轮的人。
        responder: Id,
    },
    Decision {
        call_id: String,
        approved: bool,
        responder: Id,
    },
}

impl RunResponse {
    pub fn call_id(&self) -> &str {
        match self {
            Self::Answer { call_id, .. } | Self::Decision { call_id, .. } => call_id,
        }
    }

    /// 对得上在等的那个问题或确认。
    pub fn answers(&self, pending: &Pending) -> bool {
        self.call_id() == pending.call_id()
            && matches!(
                (self, pending),
                (Self::Answer { .. }, Pending::Question { .. })
                    | (Self::Decision { .. }, Pending::Confirmation { .. })
            )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub id: Id,
    pub org_id: Id,
    pub chat_id: Id,
    pub source: RunSource,
    pub status: RunStatus,
    pub principal: RunPrincipal,
    /// 执行这一轮要用的请求：模式、时间、模型、Agent 配置、命令带来的指令……
    pub request: Value,
    pub user_message_id: Option<Id>,
    pub assistant_message_id: Option<Id>,
    /// 执行它的节点。
    pub node: Option<String>,
    pub lease_expires_at: Option<TimestampMicros>,
    pub cancel_requested: bool,
    pub pending: Option<Pending>,
    pub response: Option<RunResponse>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// 用模型服务选的币种记；没有单价时为空。
    pub cost: Option<Cost>,
    pub error: Option<String>,
    /// 最后一个事件的序号。
    pub last_seq: i64,
    pub created_at: TimestampMicros,
    pub started_at: Option<TimestampMicros>,
    pub finished_at: Option<TimestampMicros>,
    pub updated_at: TimestampMicros,
}

impl Run {
    /// 发起这一轮的人。
    pub fn initiator(&self) -> &Id {
        &self.principal.user_id
    }

    pub fn queued(
        org_id: Id,
        chat_id: Id,
        source: RunSource,
        principal: RunPrincipal,
        request: Value,
        user_message_id: Option<Id>,
    ) -> Self {
        let now = TimestampMicros::now();

        Self {
            id: Id::new(),
            org_id,
            chat_id,
            source,
            status: RunStatus::Queued,
            principal,
            request,
            user_message_id,
            assistant_message_id: None,
            node: None,
            lease_expires_at: None,
            cancel_requested: false,
            pending: None,
            response: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            cost: None,
            error: None,
            last_seq: 0,
            created_at: now,
            started_at: None,
            finished_at: None,
            updated_at: now,
        }
    }
}

/// 一轮回答怎么结束的。
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    pub status: RunStatus,
    pub assistant_message_id: Option<Id>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub cost: Option<Cost>,
    pub error: Option<String>,
}

#[async_trait]
pub trait RunRepository: Send + Sync {
    /// 排进队列，并叫醒等着领取的节点。
    async fn create(&self, run: Run) -> Result<Run>;
    async fn get(&self, org_id: &Id, id: &Id) -> Result<Run>;
    /// 一个会话最近的 Run，新的在前。
    async fn for_chat(&self, org_id: &Id, chat_id: &Id, limit: i64) -> Result<Vec<Run>>;
    /// 领走最早的一个能执行的 Run：同一会话里前面的都结束了才轮到它。
    async fn claim_next(&self, node: &str, lease_until: TimestampMicros) -> Result<Option<Run>>;
    /// 等到可能有 Run 可领（有 Run 排进队列或结束时会被叫醒），最多等 `timeout`。
    async fn wait_for_pending(&self, timeout: Duration) -> bool;
    /// 订阅这个节点里关于 Run 的提醒：新事件、停止、回答。
    fn signals(&self) -> RunSignals;
    /// 续租；Run 已不归这个节点或已结束时返回 false。
    async fn renew(&self, id: &Id, node: &str, lease_until: TimestampMicros) -> Result<bool>;
    /// 开始等提问人：记下在等什么，状态换成对应的等待。
    async fn wait_for(&self, id: &Id, pending: &Pending) -> Result<()>;
    /// 提问人的回答或决定；对不上在等的东西时报冲突。
    async fn respond(&self, org_id: &Id, id: &Id, response: &RunResponse) -> Result<Run>;
    /// 取走回答，状态回到执行中；还没有回答时返回 `None`。
    async fn take_response(&self, id: &Id) -> Result<Option<RunResponse>>;
    /// 不再等了（等太久）：清掉在等的东西，状态回到执行中；之后到的回答被拒绝。
    async fn stop_waiting(&self, id: &Id) -> Result<()>;
    /// 停止：排队中的直接结束，执行中的记下请求，由执行它的节点停下。
    async fn request_cancel(&self, org_id: &Id, id: &Id) -> Result<Run>;
    async fn cancel_requested(&self, id: &Id) -> Result<bool>;
    /// 追加事件，返回最后一个的序号，并提醒在续读的连接。Run 已经结束时报冲突。
    async fn append_events(&self, org_id: &Id, run_id: &Id, events: &[NewRunEvent]) -> Result<i64>;
    /// 结束：追加最后的事件（以 [`RunEventKind::Done`] 结尾）并记下结果。Run 已经结束（例如
    /// 已被标为中断）时什么都不写，返回 false。
    async fn finish(&self, run: &Run, outcome: &RunOutcome, events: &[NewRunEvent])
    -> Result<bool>;
    async fn events_after(&self, run_id: &Id, after: i64, limit: i64) -> Result<Vec<RunEvent>>;
    /// 租约过期的 Run 标为中断，各补一个结束事件；返回这些 Run。
    async fn expire_leases(&self, now: TimestampMicros) -> Result<Vec<Run>>;
    /// 删掉早于 `before` 的事件，返回删了多少。
    async fn purge_events(&self, before: TimestampMicros) -> Result<u64>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip_and_say_whether_they_are_over() {
        for status in [
            RunStatus::Queued,
            RunStatus::Running,
            RunStatus::WaitingInput,
            RunStatus::WaitingApproval,
            RunStatus::Succeeded,
            RunStatus::Failed,
            RunStatus::Cancelled,
            RunStatus::Interrupted,
        ] {
            assert_eq!(RunStatus::parse(status.as_str()), Some(status));
            assert!(!(status.is_terminal() && status.is_live()));
        }

        assert!(RunStatus::Interrupted.is_terminal());
        assert!(RunStatus::WaitingApproval.is_live());
        assert!(!RunStatus::Queued.is_live());
    }

    #[test]
    fn a_response_must_answer_what_is_waited_for() {
        let question = Pending::Question {
            call_id: "call-1".into(),
            question: "Which environment?".into(),
            kind: QuestionKind::Single,
            options: vec!["staging".into(), "production".into()],
        };

        let answer = RunResponse::Answer {
            call_id: "call-1".into(),
            choices: vec!["production".into()],
            text: String::new(),
            responder: Id::from_string("u1"),
        };

        let decision = RunResponse::Decision {
            call_id: "call-1".into(),
            approved: true,
            responder: Id::from_string("u1"),
        };

        assert!(answer.answers(&question));
        assert!(!decision.answers(&question));
        assert_eq!(question.status(), RunStatus::WaitingInput);

        let other = RunResponse::Answer {
            call_id: "call-2".into(),
            choices: Vec::new(),
            text: "yes".into(),
            responder: Id::from_string("u1"),
        };

        assert!(!other.answers(&question));
    }

    #[test]
    fn the_principal_keeps_the_credential_it_came_with() {
        let principal = RunPrincipal {
            user_id: Id::from_string("u1"),
            scope: IamScope::ApiToken,
            credential_role_id: Some(Id::from_string("role-1")),
            credential_application_id: None,
            credential_service_account_id: None,
        };

        let context = principal.unresolved_context(&Id::from_string("org-1"));

        assert_eq!(RunPrincipal::of(&context), principal);
        assert!(context.permissions.is_empty());
    }
}
