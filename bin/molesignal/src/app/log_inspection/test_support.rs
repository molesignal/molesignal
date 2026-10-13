// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 单测用的替身：内存里的巡检与执行记录、给定结论的模型、记下每条通知的渠道。
//!
//! 内存仓储守的规矩与 Pg 实装一样：组织内不重名，同一条巡检至多一条没结束的执行，结束一次
//! 执行同时改写巡检的「最近一次执行」。

use std::{collections::BTreeSet, sync::Arc};

use async_trait::async_trait;
use parking_lot::Mutex;

use super::{
    AgentAnswer, AgentFailure, AgentRequest, InspectionAgent, NotificationSender, RunIdentity,
    identity::RUN_PERMISSIONS, notification::LANGUAGE_KEY_PREFIX,
};
use crate::{
    domain::{
        i18n_language::{I18nLanguageRepository, LanguagePack},
        iam::{IamContext, IamScope},
        log_inspection::{
            InspectionNotification, InspectionNotifyRoute, LastInspectionRun, LogInspection,
            LogInspectionRun, RunCompletion, RunStatus,
            repositories::{LogInspectionRepository, LogInspectionRunRepository},
        },
        notify::connector::NotifyMessage,
        stream::{Schema, StreamDefinition, StreamRepository, StreamType},
    },
    shared::{Error, Result, i18n_language_seed::seeded_pack, ids::Id, time::TimestampMicros},
};

/// 巡检和执行记录都在内存里。
#[derive(Default)]
pub(crate) struct Store {
    inspections: Mutex<Vec<LogInspection>>,
    runs: Mutex<Vec<LogInspectionRun>>,
}

impl Store {
    pub(crate) fn with(inspections: impl IntoIterator<Item = LogInspection>) -> Arc<Self> {
        Arc::new(Self {
            inspections: Mutex::new(inspections.into_iter().collect()),
            runs: Mutex::default(),
        })
    }

    pub(crate) fn inspection(&self, id: &str) -> LogInspection {
        self.inspections
            .lock()
            .iter()
            .find(|inspection| inspection.id.0 == id)
            .cloned()
            .expect("inspection is stored")
    }

    pub(crate) fn inspection_count(&self) -> usize {
        self.inspections.lock().len()
    }

    pub(crate) fn runs(&self) -> Vec<LogInspectionRun> {
        self.runs.lock().clone()
    }

    pub(crate) fn run(&self, id: &Id) -> LogInspectionRun {
        self.runs
            .lock()
            .iter()
            .find(|run| run.id == *id)
            .cloned()
            .expect("run is stored")
    }

    /// 直接放进一条执行记录，不经过规矩的检查：用来摆出「节点中途退出」之类的现场。
    pub(crate) fn put_run(&self, run: LogInspectionRun) {
        self.runs.lock().push(run);
    }

    fn insert_run(&self, run: LogInspectionRun) -> Result<LogInspectionRun> {
        let mut runs = self.runs.lock();

        let active = runs
            .iter()
            .any(|other| other.inspection_id == run.inspection_id && other.status.is_active());

        if active {
            return Err(Error::conflict("uq_log_inspection_runs_active"));
        }

        runs.push(run.clone());
        Ok(run)
    }
}

fn missing() -> Error {
    Error::not_found("log inspection")
}

#[async_trait]
impl LogInspectionRepository for Store {
    async fn create(&self, inspection: LogInspection) -> Result<LogInspection> {
        let mut inspections = self.inspections.lock();

        let taken = inspections
            .iter()
            .any(|other| other.org_id == inspection.org_id && other.name == inspection.name);

        if taken {
            return Err(Error::conflict("uq_log_inspections_org_name"));
        }

        inspections.push(inspection.clone());
        Ok(inspection)
    }

    async fn update(&self, inspection: LogInspection) -> Result<LogInspection> {
        let mut inspections = self.inspections.lock();

        let taken = inspections.iter().any(|other| {
            other.id != inspection.id
                && other.org_id == inspection.org_id
                && other.name == inspection.name
        });

        if taken {
            return Err(Error::conflict("uq_log_inspections_org_name"));
        }

        let stored = inspections
            .iter_mut()
            .find(|other| other.id == inspection.id && other.org_id == inspection.org_id)
            .ok_or_else(missing)?;

        // 定义照存，最近一次执行的状况不动。
        *stored = LogInspection {
            last_run: stored.last_run.clone(),
            ..inspection
        };

        Ok(stored.clone())
    }

    async fn get(&self, org_id: &Id, id: &Id) -> Result<LogInspection> {
        self.inspections
            .lock()
            .iter()
            .find(|inspection| inspection.org_id == *org_id && inspection.id == *id)
            .cloned()
            .ok_or_else(missing)
    }

    async fn get_by_id(&self, id: &Id) -> Result<LogInspection> {
        self.inspections
            .lock()
            .iter()
            .find(|inspection| inspection.id == *id)
            .cloned()
            .ok_or_else(missing)
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<LogInspection>> {
        let mut listed = self
            .inspections
            .lock()
            .iter()
            .filter(|inspection| inspection.org_id == *org_id)
            .cloned()
            .collect::<Vec<_>>();

        listed.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(listed)
    }

    async fn count(&self, org_id: &Id) -> Result<i64> {
        let count = self
            .inspections
            .lock()
            .iter()
            .filter(|inspection| inspection.org_id == *org_id)
            .count();

        Ok(i64::try_from(count).unwrap_or(i64::MAX))
    }

    async fn delete(&self, org_id: &Id, id: &Id) -> Result<()> {
        let mut inspections = self.inspections.lock();
        let before = inspections.len();
        inspections.retain(|inspection| !(inspection.org_id == *org_id && inspection.id == *id));

        if inspections.len() == before {
            return Err(missing());
        }

        self.runs.lock().retain(|run| run.inspection_id != *id);
        Ok(())
    }

    async fn set_enabled(
        &self,
        org_id: &Id,
        id: &Id,
        enabled: bool,
        next_run_at: Option<TimestampMicros>,
        now: TimestampMicros,
    ) -> Result<LogInspection> {
        let mut inspections = self.inspections.lock();

        let stored = inspections
            .iter_mut()
            .find(|inspection| inspection.org_id == *org_id && inspection.id == *id)
            .ok_or_else(missing)?;

        stored.enabled = enabled;
        stored.next_run_at = next_run_at;
        stored.updated_at = now;
        Ok(stored.clone())
    }

    async fn list_due(&self, now: TimestampMicros, limit: i64) -> Result<Vec<LogInspection>> {
        let mut due = self
            .inspections
            .lock()
            .iter()
            .filter(|inspection| {
                inspection.enabled && inspection.next_run_at.is_some_and(|at| at <= now)
            })
            .cloned()
            .collect::<Vec<_>>();

        due.sort_by_key(|inspection| inspection.next_run_at);
        due.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        Ok(due)
    }
}

#[async_trait]
impl LogInspectionRunRepository for Store {
    async fn begin_scheduled(
        &self,
        run: LogInspectionRun,
        next_run_at: Option<TimestampMicros>,
    ) -> Result<LogInspectionRun> {
        let run = self.insert_run(run)?;

        if let Some(inspection) = self
            .inspections
            .lock()
            .iter_mut()
            .find(|inspection| inspection.id == run.inspection_id)
        {
            inspection.next_run_at = next_run_at;
        }

        Ok(run)
    }

    async fn enqueue(&self, run: LogInspectionRun) -> Result<LogInspectionRun> {
        self.insert_run(run)
    }

    async fn list_queued(&self, limit: i64) -> Result<Vec<LogInspectionRun>> {
        let mut queued = self
            .runs
            .lock()
            .iter()
            .filter(|run| run.status == RunStatus::Queued)
            .cloned()
            .collect::<Vec<_>>();

        queued.sort_by_key(|run| run.created_at);
        queued.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        Ok(queued)
    }

    async fn start_queued(
        &self,
        run_id: &Id,
        started_at: TimestampMicros,
        window_start: TimestampMicros,
        window_end: TimestampMicros,
    ) -> Result<bool> {
        let mut runs = self.runs.lock();

        let Some(run) = runs
            .iter_mut()
            .find(|run| run.id == *run_id && run.status == RunStatus::Queued)
        else {
            return Ok(false);
        };

        run.status = RunStatus::Running;
        run.started_at = Some(started_at);
        run.window_start = window_start;
        run.window_end = window_end;
        Ok(true)
    }

    async fn finish(&self, run_id: &Id, completion: RunCompletion) -> Result<bool> {
        let mut runs = self.runs.lock();

        let Some(run) = runs
            .iter_mut()
            .find(|run| run.id == *run_id && run.status.is_active())
        else {
            return Ok(false);
        };

        run.status = completion.status;
        run.level = completion.level;
        run.summary = completion.summary;
        run.report = completion.report;
        run.failure = completion.failure;
        run.error = completion.error;
        run.provider_id = completion.provider_id;
        run.model = completion.model;
        run.run_as_user_id = completion.run_as_user_id;
        run.tool_calls = completion.tool_calls;
        run.prompt_tokens = completion.prompt_tokens;
        run.completion_tokens = completion.completion_tokens;
        run.finished_at = Some(completion.finished_at);

        if let Some(inspection) = self
            .inspections
            .lock()
            .iter_mut()
            .find(|inspection| inspection.id == run.inspection_id)
        {
            inspection.last_run = Some(LastInspectionRun {
                at: completion.finished_at,
                status: completion.status,
                level: completion.level,
            });
        }

        Ok(true)
    }

    async fn record_notifications(
        &self,
        run_id: &Id,
        notifications: &[InspectionNotification],
    ) -> Result<()> {
        if let Some(run) = self.runs.lock().iter_mut().find(|run| run.id == *run_id) {
            run.notifications = notifications.to_vec();
        }

        Ok(())
    }

    async fn list_stale(
        &self,
        running_before: TimestampMicros,
        queued_before: TimestampMicros,
    ) -> Result<Vec<LogInspectionRun>> {
        Ok(self
            .runs
            .lock()
            .iter()
            .filter(|run| match run.status {
                RunStatus::Running => run.started_at.is_some_and(|at| at < running_before),
                RunStatus::Queued => run.created_at < queued_before,
                _ => false,
            })
            .cloned()
            .collect())
    }

    async fn list_active(&self, org_id: &Id) -> Result<Vec<LogInspectionRun>> {
        Ok(self
            .runs
            .lock()
            .iter()
            .filter(|run| run.org_id == *org_id && run.status.is_active())
            .cloned()
            .collect())
    }

    async fn list(
        &self,
        org_id: &Id,
        inspection_id: &Id,
        before: Option<TimestampMicros>,
        limit: i64,
    ) -> Result<Vec<LogInspectionRun>> {
        let mut listed = self
            .runs
            .lock()
            .iter()
            .filter(|run| run.org_id == *org_id && run.inspection_id == *inspection_id)
            .filter(|run| before.is_none_or(|before| run.created_at < before))
            .map(|run| LogInspectionRun {
                report: None,
                ..run.clone()
            })
            .collect::<Vec<_>>();

        listed.sort_by_key(|run| std::cmp::Reverse(run.created_at));
        listed.truncate(usize::try_from(limit).unwrap_or(usize::MAX));
        Ok(listed)
    }

    async fn get(&self, org_id: &Id, inspection_id: &Id, run_id: &Id) -> Result<LogInspectionRun> {
        self.runs
            .lock()
            .iter()
            .find(|run| {
                run.org_id == *org_id && run.inspection_id == *inspection_id && run.id == *run_id
            })
            .cloned()
            .ok_or_else(|| Error::not_found("log inspection run"))
    }

    async fn delete_before(&self, cutoff: TimestampMicros) -> Result<u64> {
        let mut runs = self.runs.lock();
        let before = runs.len();
        runs.retain(|run| run.status.is_active() || run.created_at >= cutoff);

        Ok(u64::try_from(before - runs.len()).unwrap_or(u64::MAX))
    }
}

/// 一位此刻持有这些权限的组织成员。
pub(crate) fn member(user_id: &str, permissions: &[&str]) -> IamContext {
    IamContext {
        user_id: Id::from_string(user_id),
        org_id: Id::from_string("org-1"),
        display_role: String::new(),
        roles: Vec::new(),
        credential_role_id: None,
        credential_application_id: None,
        credential_service_account_id: None,
        scope: IamScope::Organization,
        permissions: permissions
            .iter()
            .map(|key| (*key).to_string())
            .collect::<BTreeSet<_>>(),
        policy_version: 0,
    }
}

/// IAM 对「巡检记着的那位用户此刻是谁」的回答。
pub(crate) enum Identity {
    /// 还在，权限够。
    Usable,
    /// 用不了了，附原因。
    Unusable(&'static str),
    /// IAM 自己出了故障。
    Unreachable,
}

#[async_trait]
impl RunIdentity for Identity {
    async fn resolve(&self, _org_id: &Id, user_id: &Id) -> Result<IamContext> {
        match self {
            Self::Usable => Ok(member(&user_id.0, &RUN_PERMISSIONS)),
            Self::Unusable(reason) => Err(Error::forbidden(*reason)),
            Self::Unreachable => Err(Error::internal("iam database is unreachable")),
        }
    }
}

/// 模型会怎么回答。
pub(crate) enum Model {
    /// 写出这份报告，其间调用过这些工具（名字，是否成功）。
    Reports(&'static str, &'static [(&'static str, bool)]),
    /// 模型配置用不了。
    Unusable,
    /// 调用失败。
    Fails,
    /// 一直不回来。
    Hangs,
}

/// 记下每次巡检请求的模型替身。
pub(crate) struct Agent {
    model: Model,
    requests: Mutex<Vec<(String, String)>>,
}

impl Agent {
    pub(crate) fn new(model: Model) -> Arc<Self> {
        Arc::new(Self {
            model,
            requests: Mutex::default(),
        })
    }

    /// 被要求巡检过的（数据流，任务）。
    pub(crate) fn requests(&self) -> Vec<(String, String)> {
        self.requests.lock().clone()
    }
}

#[async_trait]
impl InspectionAgent for Agent {
    async fn check_model(
        &self,
        _org_id: &Id,
        provider_id: &Id,
    ) -> std::result::Result<(), AgentFailure> {
        if matches!(self.model, Model::Unusable) || provider_id.0 == "missing" {
            return Err(AgentFailure::Model("model provider is disabled".into()));
        }

        Ok(())
    }

    async fn inspect(
        &self,
        request: AgentRequest,
    ) -> std::result::Result<AgentAnswer, AgentFailure> {
        self.requests
            .lock()
            .push((request.stream.clone(), request.task.clone()));

        match &self.model {
            Model::Reports(report, tools) => {
                for (name, ok) in *tools {
                    request.tool_calls.record(name, *ok);
                }

                Ok(AgentAnswer {
                    content: (*report).to_string(),
                    model: request.model.unwrap_or_else(|| "default-model".into()),
                    prompt_tokens: 1_200,
                    completion_tokens: 300,
                })
            }
            Model::Unusable => Err(AgentFailure::Model("model provider is disabled".into())),
            Model::Fails => Err(AgentFailure::Call("provider returned HTTP 500".into())),
            Model::Hangs => {
                // 调用了一个工具之后再也不回来。
                request.tool_calls.record("query_logs", true);
                std::future::pending().await
            }
        }
    }
}

/// 记下每条通知的渠道替身。
#[derive(Default)]
pub(crate) struct Channels {
    /// 这个连接器不存在。
    pub(crate) missing_connector: Option<&'static str>,
    /// 连接器收下了但没送达。
    pub(crate) undelivered: bool,
    pub(crate) sent: Mutex<Vec<(Id, InspectionNotifyRoute, NotifyMessage)>>,
}

impl Channels {
    pub(crate) fn sent(&self) -> Vec<(Id, InspectionNotifyRoute, NotifyMessage)> {
        self.sent.lock().clone()
    }
}

#[async_trait]
impl NotificationSender for Channels {
    async fn check(&self, _org_id: &Id, route: &InspectionNotifyRoute) -> Result<()> {
        if self.missing_connector == Some(route.connector_id.0.as_str()) {
            return Err(Error::not_found("notify connector"));
        }

        Ok(())
    }

    async fn send(
        &self,
        _org_id: &Id,
        run_id: &Id,
        route: &InspectionNotifyRoute,
        message: &NotifyMessage,
    ) -> InspectionNotification {
        self.sent
            .lock()
            .push((run_id.clone(), route.clone(), message.clone()));

        InspectionNotification {
            connector_id: route.connector_id.clone(),
            target_type: route.target_type,
            target_masked: "oc***ps".into(),
            delivered: !self.undelivered,
            error: self.undelivered.then(|| "webhook returned HTTP 502".into()),
        }
    }
}

/// 一个刚建好的数据库里的语言条目。
pub(crate) struct SeededLanguage;

#[async_trait]
impl I18nLanguageRepository for SeededLanguage {
    async fn load_pack(&self, key_prefix: &str, language_code: &str) -> Result<LanguagePack> {
        assert_eq!(key_prefix, LANGUAGE_KEY_PREFIX);
        Ok(seeded_pack(key_prefix, language_code))
    }
}

/// 组织里有这些日志数据流。
pub(crate) struct Streams(pub(crate) &'static [&'static str]);

#[async_trait]
impl StreamRepository for Streams {
    async fn create(&self, def: StreamDefinition) -> Result<StreamDefinition> {
        Ok(def)
    }

    async fn update_schema(&self, _id: &Id, _schema: Schema) -> Result<()> {
        Ok(())
    }

    async fn get(
        &self,
        org_id: &Id,
        name: &str,
        stream_type: StreamType,
    ) -> Result<StreamDefinition> {
        if stream_type != StreamType::LOGS || !self.0.contains(&name) {
            return Err(Error::not_found(format!("stream {name}")));
        }

        Ok(StreamDefinition {
            id: Id::from_string(format!("stream-{name}")),
            org_id: org_id.clone(),
            name: name.to_string(),
            stream_type,
            schema: Schema { fields: Vec::new() },
            retention: None,
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        })
    }

    async fn list(&self, _org_id: &Id) -> Result<Vec<StreamDefinition>> {
        Ok(Vec::new())
    }

    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}
