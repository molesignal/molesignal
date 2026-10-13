// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一次巡检从开始到结束：还原执行身份 → 让模型巡检（带总超时）→ 读出结论等级 → 落报告或
//! 失败原因 → 按规则通知。
//!
//! 模型的 HTTP client 没有超时，一次巡检又要来回调用模型和工具多次，所以总时间在这里统一
//! 限定：超时就丢弃这次执行并记为超时，不动别人共用的 client。失败不重试，等下一个周期。

use std::{
    sync::{Arc, OnceLock},
    time::Instant,
};

use chrono_tz::Tz;
use prometheus::{Histogram, IntCounterVec};

use super::{
    InspectionLimits,
    agent::{AgentFailure, AgentRequest, InspectionAgent, ToolCallLog},
    identity::RunIdentity,
    notification::{self, NotificationSender, Outcome},
    prompt, schedule,
    verdict::read_verdict,
};
use crate::{
    domain::{
        i18n_language::I18nLanguageRepository,
        log_inspection::{
            InspectionLevel, InspectionNotification, LastInspectionRun, LogInspection,
            LogInspectionRun, RunCompletion, RunFailure, RunStatus,
            repositories::{LogInspectionRepository, LogInspectionRunRepository},
        },
    },
    shared::{
        Error, Result,
        metrics::{register_histogram, register_int_counter_vec},
        time::TimestampMicros,
    },
};

mod trigger;

/// 失败的技术细节最多留这么多字符。
const ERROR_CHARS: usize = 2_000;

/// 一次执行的结局，供调用方记日志。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub status: RunStatus,
    pub level: Option<InspectionLevel>,
    /// 发出去的通知条数（含没送达的）。
    pub notifications: usize,
}

pub struct InspectionRunner {
    inspections: Arc<dyn LogInspectionRepository>,
    runs: Arc<dyn LogInspectionRunRepository>,
    identity: Arc<dyn RunIdentity>,
    agent: Arc<dyn InspectionAgent>,
    sender: Arc<dyn NotificationSender>,
    i18n_language: Arc<dyn I18nLanguageRepository>,
    /// 通知里报告链接的前缀；空表示实例没有配置对外地址。
    external_url: String,
    limits: InspectionLimits,
}

/// 模型这一段的结果：报告，或者没有报告的原因。
enum Attempt {
    Report {
        content: String,
        model: String,
        prompt_tokens: i64,
        completion_tokens: i64,
    },
    Failed {
        status: RunStatus,
        failure: RunFailure,
        error: String,
    },
}

impl Attempt {
    fn failed(failure: RunFailure, error: impl std::fmt::Display) -> Self {
        Self::Failed {
            status: RunStatus::Failed,
            failure,
            error: error.to_string().chars().take(ERROR_CHARS).collect(),
        }
    }
}

impl InspectionRunner {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        inspections: Arc<dyn LogInspectionRepository>,
        runs: Arc<dyn LogInspectionRunRepository>,
        identity: Arc<dyn RunIdentity>,
        agent: Arc<dyn InspectionAgent>,
        sender: Arc<dyn NotificationSender>,
        i18n_language: Arc<dyn I18nLanguageRepository>,
        external_url: String,
        limits: InspectionLimits,
    ) -> Self {
        Self {
            inspections,
            runs,
            identity,
            agent,
            sender,
            i18n_language,
            external_url,
            limits,
        }
    }

    pub fn limits(&self) -> InspectionLimits {
        self.limits
    }

    /// 执行一条已经记为「执行中」的记录，直到它结束并按规则发出通知。
    ///
    /// `inspection` 是开始这次执行之前读到的巡检：它的「最近一次执行」是上一次的结果，
    /// 用来判断这次失败是不是由好转坏。
    #[tracing::instrument(
        name = "log_inspection.run",
        skip_all,
        fields(otel.kind = "internal", molesignal.log_inspection.trigger = run.trigger.as_str())
    )]
    pub async fn execute(&self, inspection: &LogInspection, run: &LogInspectionRun) -> RunOutcome {
        let started = Instant::now();
        let tool_calls = Arc::new(ToolCallLog::default());
        let attempt = self.attempt(inspection, run, tool_calls.clone()).await;
        run_duration().observe(started.elapsed().as_secs_f64());

        self.conclude(inspection, run, attempt, &tool_calls).await
    }

    /// 把一条卡住的记录记为失败：执行它的节点中途退出了，或排队后一直没有节点来执行。
    /// 记录已经被别处结束时什么都不做。
    pub async fn abandon(&self, run: &LogInspectionRun) -> Result<Option<RunOutcome>> {
        let attempt = if run.status == RunStatus::Queued {
            Attempt::failed(RunFailure::NotStarted, "no node picked up this run")
        } else {
            Attempt::failed(
                RunFailure::Interrupted,
                "the node running this inspection stopped before it finished",
            )
        };

        let inspection = match self.inspections.get_by_id(&run.inspection_id).await {
            Ok(inspection) => inspection,
            // 巡检已被删除，记录随之消失。
            Err(Error::NotFound(_)) => return Ok(None),
            Err(error) => return Err(error),
        };

        let outcome = self
            .conclude(&inspection, run, attempt, &ToolCallLog::default())
            .await;

        Ok(Some(outcome))
    }

    async fn attempt(
        &self,
        inspection: &LogInspection,
        run: &LogInspectionRun,
        tool_calls: Arc<ToolCallLog>,
    ) -> Attempt {
        let identity = match self
            .identity
            .resolve(&inspection.org_id, &inspection.run_as_user_id)
            .await
        {
            Ok(identity) => identity,
            Err(Error::Forbidden(reason)) => return Attempt::failed(RunFailure::Identity, reason),
            Err(error) => return Attempt::failed(RunFailure::Internal, error),
        };

        let prompt = prompt::build(
            inspection,
            timezone(inspection),
            run.window_start,
            run.window_end,
            TimestampMicros::now(),
        );

        let request = AgentRequest {
            identity,
            provider_id: inspection.provider_id.clone(),
            model: inspection.model.clone(),
            stream: inspection.stream.clone(),
            instructions: prompt.instructions,
            task: prompt.task,
            tool_calls,
        };

        // 超时即丢弃这个 future：正在等待的模型请求随之中断，不会再有后续的调用。
        match tokio::time::timeout(self.limits.run_timeout, self.agent.inspect(request)).await {
            Ok(Ok(answer)) => Attempt::Report {
                content: answer.content,
                model: answer.model,
                prompt_tokens: answer.prompt_tokens,
                completion_tokens: answer.completion_tokens,
            },
            Ok(Err(AgentFailure::Model(error))) => Attempt::failed(RunFailure::Model, error),
            Ok(Err(AgentFailure::Call(error))) => Attempt::failed(RunFailure::Agent, error),
            Err(_) => Attempt::Failed {
                status: RunStatus::TimedOut,
                failure: RunFailure::TimedOut,
                error: format!(
                    "the inspection did not finish within {} seconds",
                    self.limits.run_timeout.as_secs()
                ),
            },
        }
    }

    /// 落结果，再按规则通知。记录已经被别处结束时两件事都不做。
    async fn conclude(
        &self,
        inspection: &LogInspection,
        run: &LogInspectionRun,
        attempt: Attempt,
        tool_calls: &ToolCallLog,
    ) -> RunOutcome {
        let finished_at = TimestampMicros::now();

        let mut completion = RunCompletion {
            status: RunStatus::Succeeded,
            level: None,
            summary: None,
            report: None,
            failure: None,
            error: None,
            provider_id: Some(inspection.provider_id.clone()),
            model: inspection.model.clone(),
            run_as_user_id: Some(inspection.run_as_user_id.clone()),
            tool_calls: tool_calls.calls(),
            prompt_tokens: 0,
            completion_tokens: 0,
            finished_at,
        };

        match attempt {
            Attempt::Report {
                content,
                model,
                prompt_tokens,
                completion_tokens,
            } => {
                let verdict = read_verdict(&content);

                completion.level = Some(verdict.level);
                completion.summary = verdict.summary;
                completion.report = Some(content);
                completion.model = Some(model);
                completion.prompt_tokens = prompt_tokens;
                completion.completion_tokens = completion_tokens;
            }
            Attempt::Failed {
                status,
                failure,
                error,
            } => {
                completion.status = status;
                completion.failure = Some(failure);
                completion.error = Some(error);
            }
        }

        let status = completion.status;
        let level = completion.level;
        let failure = completion.failure;
        let summary = completion.summary.clone();

        let outcome = RunOutcome {
            status,
            level,
            notifications: 0,
        };

        match self.runs.finish(&run.id, completion).await {
            Ok(true) => {}
            Ok(false) => return outcome,
            Err(error) => {
                tracing::warn!(
                    run_id = %run.id.0,
                    error = %error,
                    "log inspection run result could not be saved"
                );
                return outcome;
            }
        }

        runs_total().with_label_values(&[status.as_str()]).inc();

        let told = match (level, failure) {
            (Some(level), _) => Outcome::Report {
                level,
                summary: summary.as_deref(),
            },
            (None, Some(failure)) => Outcome::Failure(failure),
            (None, None) => return outcome,
        };

        let notifications = self
            .notify(inspection, run, inspection.last_run.as_ref(), told)
            .await;

        RunOutcome {
            notifications,
            ..outcome
        }
    }

    /// 发通知并把每个渠道的结果记到执行记录上，返回尝试发送的条数。
    async fn notify(
        &self,
        inspection: &LogInspection,
        run: &LogInspectionRun,
        previous: Option<&LastInspectionRun>,
        outcome: Outcome<'_>,
    ) -> usize {
        if !notification::should_notify(inspection, previous, outcome) {
            return 0;
        }

        let url = notification::report_url(&self.external_url, run);

        let composed = match self
            .i18n_language
            .load_pack(notification::LANGUAGE_KEY_PREFIX, &inspection.language)
            .await
        {
            Ok(pack) => notification::message(
                &pack,
                inspection,
                run,
                timezone(inspection),
                outcome,
                url.as_deref(),
            ),
            Err(error) => Err(error),
        };

        let mut notifications = Vec::with_capacity(inspection.notify_routes.len());

        for route in &inspection.notify_routes {
            notifications.push(match &composed {
                Ok(message) => {
                    self.sender
                        .send(&inspection.org_id, &run.id, route, message)
                        .await
                }
                // 文案写不出来（语言条目缺失），每个渠道都记一条没发出去的。
                Err(error) => InspectionNotification {
                    connector_id: route.connector_id.clone(),
                    target_type: route.target_type,
                    target_masked: String::new(),
                    delivered: false,
                    error: Some(error.to_string()),
                },
            });
        }

        if let Err(error) = self
            .runs
            .record_notifications(&run.id, &notifications)
            .await
        {
            tracing::warn!(
                run_id = %run.id.0,
                error = %error,
                "log inspection notification results could not be saved"
            );
        }

        notifications.len()
    }
}

/// 巡检的时区。保存时校验过；读不出来时按 UTC，不让一次执行因此失败。
fn timezone(inspection: &LogInspection) -> Tz {
    schedule::parse_timezone(&inspection.timezone).unwrap_or(Tz::UTC)
}

fn runs_total() -> &'static IntCounterVec {
    static METRIC: OnceLock<IntCounterVec> = OnceLock::new();

    METRIC.get_or_init(|| {
        register_int_counter_vec(
            "log_inspection_runs_total",
            "Scheduled log inspection runs that ended, by how they ended",
            &["outcome"],
        )
    })
}

fn run_duration() -> &'static Histogram {
    static METRIC: OnceLock<Histogram> = OnceLock::new();

    METRIC.get_or_init(|| {
        register_histogram(
            "log_inspection_run_duration_seconds",
            "Time one scheduled log inspection spent with the model and its tools",
            vec![5.0, 15.0, 30.0, 60.0, 120.0, 180.0, 300.0, 480.0],
        )
    })
}

#[cfg(test)]
mod tests;
