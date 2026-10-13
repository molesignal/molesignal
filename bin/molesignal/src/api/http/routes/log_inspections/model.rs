// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 定时巡检接口的请求体与响应体。时间一律是 Unix 微秒，字段名带 `_micros`。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::{
        log_inspection::{InspectionInput, InspectionLimits},
        reporting::{RunAsProfile, run_as_profiles},
    },
    domain::log_inspection::{
        InspectionLevel, InspectionNotification, InspectionNotifyRoute, InspectionSchedule,
        InspectionToolCall, LogInspection, LogInspectionRun, NotifyWhen, RunFailure, RunStatus,
        RunTrigger,
    },
    shared::ids::Id,
};

/// 创建和修改用同一个请求体：每次保存都是完整的定义。
#[derive(Debug, Deserialize)]
pub(super) struct SaveReq {
    pub name: String,
    pub stream: String,
    #[serde(default)]
    pub statement: String,
    pub window_secs: i64,
    #[serde(default)]
    pub requirements: String,
    pub schedule: InspectionSchedule,
    pub timezone: String,
    pub provider_id: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub notify_when: NotifyWhen,
    #[serde(default)]
    pub notify_routes: Vec<InspectionNotifyRoute>,
    pub language: String,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
}

fn enabled_by_default() -> bool {
    true
}

impl From<SaveReq> for InspectionInput {
    fn from(req: SaveReq) -> Self {
        Self {
            name: req.name,
            stream: req.stream,
            statement: req.statement,
            window_secs: req.window_secs,
            requirements: req.requirements,
            schedule: req.schedule,
            timezone: req.timezone,
            provider_id: req.provider_id,
            model: req.model,
            notify_when: req.notify_when,
            notify_routes: req.notify_routes,
            language: req.language,
            enabled: req.enabled,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct LastRunResp {
    pub at_micros: i64,
    pub status: RunStatus,
    /// 只有成功的执行才有结论等级。
    pub level: Option<InspectionLevel>,
}

/// 这条巡检此刻排队中或执行中的那一次。
#[derive(Debug, Serialize)]
pub(super) struct ActiveRunResp {
    pub id: String,
    pub status: RunStatus,
    pub trigger: RunTrigger,
    pub created_at_micros: i64,
    pub started_at_micros: Option<i64>,
}

#[derive(Debug, Serialize)]
pub(super) struct InspectionResp {
    pub id: String,
    pub name: String,
    pub stream: String,
    pub statement: String,
    pub window_secs: i64,
    pub requirements: String,
    pub schedule: InspectionSchedule,
    pub timezone: String,
    pub provider_id: String,
    /// `null`：每次执行时用该模型配置的默认模型。
    pub model: Option<String>,
    pub notify_when: NotifyWhen,
    pub notify_routes: Vec<InspectionNotifyRoute>,
    pub language: String,
    pub enabled: bool,
    /// 每次执行都以这位用户当时的权限读取日志：最后一次保存巡检的登录用户。
    pub run_as_user_id: String,
    /// 这位用户的显示名和邮箱。用户记录已经不存在时为 `null`。
    pub run_as_user: Option<RunAsProfile>,
    /// 下一次定时执行；停用时为 `null`。
    pub next_run_at_micros: Option<i64>,
    /// 最近一次结束的执行；还没有执行过时为 `null`。
    pub last_run: Option<LastRunResp>,
    pub active_run: Option<ActiveRunResp>,
    pub created_by: String,
    pub created_at_micros: i64,
    pub updated_at_micros: i64,
}

impl InspectionResp {
    pub(super) fn new(
        inspection: LogInspection,
        active: Option<&LogInspectionRun>,
        users: &HashMap<Id, RunAsProfile>,
    ) -> Self {
        Self {
            run_as_user: users.get(&inspection.run_as_user_id).cloned(),
            last_run: inspection.last_run.map(|run| LastRunResp {
                at_micros: run.at.0,
                status: run.status,
                level: run.level,
            }),
            active_run: active.map(|run| ActiveRunResp {
                id: run.id.0.clone(),
                status: run.status,
                trigger: run.trigger,
                created_at_micros: run.created_at.0,
                started_at_micros: run.started_at.map(|at| at.0),
            }),
            id: inspection.id.0,
            name: inspection.name,
            stream: inspection.stream,
            statement: inspection.statement,
            window_secs: inspection.window_secs,
            requirements: inspection.requirements,
            schedule: inspection.schedule,
            timezone: inspection.timezone,
            provider_id: inspection.provider_id.0,
            model: inspection.model,
            notify_when: inspection.notify_when,
            notify_routes: inspection.notify_routes,
            language: inspection.language,
            enabled: inspection.enabled,
            run_as_user_id: inspection.run_as_user_id.0,
            next_run_at_micros: inspection.next_run_at.map(|at| at.0),
            created_by: inspection.created_by.0,
            created_at_micros: inspection.created_at.0,
            updated_at_micros: inspection.updated_at.0,
        }
    }
}

#[derive(Debug, Serialize)]
pub(super) struct RunResp {
    pub id: String,
    pub inspection_id: String,
    pub trigger: RunTrigger,
    pub status: RunStatus,
    pub level: Option<InspectionLevel>,
    pub window_start_micros: i64,
    pub window_end_micros: i64,
    pub summary: Option<String>,
    /// 模型写的报告原文。列表里不带，读单条时才有。
    pub report: Option<String>,
    /// 没有产出报告的原因。
    pub failure: Option<RunFailure>,
    /// 失败的技术细节。
    pub error: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub run_as_user_id: Option<String>,
    /// 手动触发的人；定时触发时为 `null`。
    pub requested_by: Option<String>,
    pub requested_by_user: Option<RunAsProfile>,
    pub tool_calls: Vec<InspectionToolCall>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub notifications: Vec<InspectionNotification>,
    pub created_at_micros: i64,
    pub started_at_micros: Option<i64>,
    pub finished_at_micros: Option<i64>,
}

impl RunResp {
    pub(super) fn new(run: LogInspectionRun, users: &HashMap<Id, RunAsProfile>) -> Self {
        Self {
            requested_by_user: run
                .requested_by
                .as_ref()
                .and_then(|id| users.get(id).cloned()),
            id: run.id.0,
            inspection_id: run.inspection_id.0,
            trigger: run.trigger,
            status: run.status,
            level: run.level,
            window_start_micros: run.window_start.0,
            window_end_micros: run.window_end.0,
            summary: run.summary,
            report: run.report,
            failure: run.failure,
            error: run.error,
            provider_id: run.provider_id.map(|id| id.0),
            model: run.model,
            run_as_user_id: run.run_as_user_id.map(|id| id.0),
            requested_by: run.requested_by.map(|id| id.0),
            tool_calls: run.tool_calls,
            prompt_tokens: run.prompt_tokens,
            completion_tokens: run.completion_tokens,
            notifications: run.notifications,
            created_at_micros: run.created_at.0,
            started_at_micros: run.started_at.map(|at| at.0),
            finished_at_micros: run.finished_at.map(|at| at.0),
        }
    }
}

/// 实例对定时巡检的限制，页面据此给出可选的周期并说明上限。
#[derive(Debug, Serialize)]
pub(super) struct LimitsResp {
    pub min_interval_secs: u64,
    pub max_per_org: u32,
    pub run_timeout_secs: u64,
    pub report_retention_days: u64,
}

impl From<InspectionLimits> for LimitsResp {
    fn from(limits: InspectionLimits) -> Self {
        Self {
            min_interval_secs: limits.min_interval_secs,
            max_per_org: limits.max_per_org,
            run_timeout_secs: limits.run_timeout.as_secs(),
            report_retention_days: limits.report_retention.as_secs() / (24 * 3600),
        }
    }
}

/// 这些用户的显示名和邮箱，按用户 id 索引。直接读用户记录：能看巡检的人不一定能读成员目录
/// （`org.members.read`）。用户表读不了时巡检照常返回，只是只剩用户 id。
pub(super) async fn profiles<'a>(
    state: &AppState,
    user_ids: impl IntoIterator<Item = &'a Id>,
) -> HashMap<Id, RunAsProfile> {
    match run_as_profiles(state.iam.service.users.as_ref(), user_ids).await {
        Ok(profiles) => profiles,
        Err(error) => {
            tracing::warn!(%error, "log inspection user profiles could not be read");
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::{
        domain::{log_inspection::LastInspectionRun, notify::connector::NotifyTargetType},
        shared::time::TimestampMicros,
    };

    fn inspection() -> LogInspection {
        LogInspection {
            id: Id::from_string("insp-1"),
            org_id: Id::from_string("org-1"),
            name: "payments".into(),
            stream: "app_logs".into(),
            statement: "level = 'ERROR'".into(),
            window_secs: 3600,
            requirements: String::new(),
            schedule: InspectionSchedule::Daily {
                hour: 9,
                minute: 30,
            },
            timezone: "Asia/Shanghai".into(),
            provider_id: Id::from_string("provider-1"),
            model: None,
            notify_when: NotifyWhen::Attention,
            notify_routes: vec![InspectionNotifyRoute {
                connector_id: Id::from_string("connector-1"),
                target_type: NotifyTargetType::FixedGroup,
                target: "oc_ops".into(),
            }],
            language: "zh-cn".into(),
            enabled: true,
            run_as_user_id: Id::from_string("user-1"),
            next_run_at: Some(TimestampMicros(2_000)),
            last_run: Some(LastInspectionRun {
                at: TimestampMicros(1_000),
                status: RunStatus::Succeeded,
                level: Some(InspectionLevel::Abnormal),
            }),
            created_by: Id::from_string("user-1"),
            created_at: TimestampMicros(10),
            updated_at: TimestampMicros(20),
        }
    }

    #[test]
    fn a_save_request_reads_the_schedule_by_its_kind_and_fills_what_is_left_out() {
        let req: SaveReq = serde_json::from_value(json!({
            "name": "payments",
            "stream": "app_logs",
            "window_secs": 3600,
            "schedule": { "type": "weekly", "weekday": 1, "hour": 9, "minute": 0 },
            "timezone": "Asia/Shanghai",
            "provider_id": "provider-1",
            "language": "zh-cn"
        }))
        .unwrap();

        let input = InspectionInput::from(req);

        assert_eq!(
            input.schedule,
            InspectionSchedule::Weekly {
                weekday: 1,
                hour: 9,
                minute: 0
            }
        );
        assert_eq!(input.notify_when, NotifyWhen::Abnormal);
        assert!(input.notify_routes.is_empty());
        assert!(input.enabled);
        assert_eq!(input.statement, "");
    }

    #[test]
    fn an_inspection_is_returned_with_who_it_runs_as_and_how_it_last_went() {
        let mut users = HashMap::new();

        users.insert(
            Id::from_string("user-1"),
            RunAsProfile {
                id: Id::from_string("user-1"),
                display_name: "Alice".into(),
                email: "alice@example.com".into(),
            },
        );

        let body = serde_json::to_value(InspectionResp::new(inspection(), None, &users)).unwrap();

        assert_eq!(
            body["schedule"],
            json!({ "type": "daily", "hour": 9, "minute": 30 })
        );
        assert_eq!(body["notify_when"], "attention");
        assert_eq!(body["notify_routes"][0]["target_type"], "fixed_group");
        assert_eq!(body["run_as_user"]["display_name"], "Alice");
        assert_eq!(body["next_run_at_micros"], 2_000);
        assert_eq!(
            body["last_run"],
            json!({ "at_micros": 1_000, "status": "succeeded", "level": "abnormal" })
        );
        assert_eq!(body["active_run"], json!(null));
        assert_eq!(body["model"], json!(null));
    }

    #[test]
    fn a_user_who_no_longer_exists_leaves_only_the_id() {
        let body =
            serde_json::to_value(InspectionResp::new(inspection(), None, &HashMap::new())).unwrap();

        assert_eq!(body["run_as_user_id"], "user-1");
        assert_eq!(body["run_as_user"], json!(null));
    }

    #[test]
    fn limits_are_told_in_seconds_and_days() {
        let body = serde_json::to_value(LimitsResp::from(InspectionLimits::new(
            3600, 20, 2, 300, 30,
        )))
        .unwrap();

        assert_eq!(
            body,
            json!({
                "min_interval_secs": 3600,
                "max_per_org": 20,
                "run_timeout_secs": 300,
                "report_retention_days": 30
            })
        );
    }
}
