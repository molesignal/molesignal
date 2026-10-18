// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 一次执行结束后要不要通知、通知写什么、往哪里发。
//!
//! 报告按巡检的「何时通知」和结论等级决定；没有产出报告的执行只在巡检由好转坏时通知一次，
//! 之后同样的失败不再重复。文案来自 `i18n_language`，用巡检保存时的语言。

use std::sync::Arc;

use async_trait::async_trait;
use chrono_tz::Tz;

use crate::{
    app::notify::{DirectSend, NotifyService},
    domain::{
        i18n_language::LanguagePack,
        log_inspection::{
            InspectionLevel, InspectionNotification, InspectionNotifyRoute, LastInspectionRun,
            LogInspection, LogInspectionRun, RunFailure,
        },
        notify::connector::{NotifyMessage, NotifyTarget},
    },
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 通知文案的每个条目都在这个前缀下。
pub const LANGUAGE_KEY_PREFIX: &str = "log_inspection.";

/// 一次执行的结果，通知只关心这两种。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome<'a> {
    Report {
        level: InspectionLevel,
        summary: Option<&'a str>,
    },
    Failure(RunFailure),
}

/// 这个结果要不要通知。`previous` 是这次执行之前最近一次结束的执行。
pub fn should_notify(
    inspection: &LogInspection,
    previous: Option<&LastInspectionRun>,
    outcome: Outcome<'_>,
) -> bool {
    if inspection.notify_routes.is_empty() {
        return false;
    }

    match outcome {
        Outcome::Report { level, .. } => inspection.notify_when.covers(level),
        // 坏掉的巡检不出声，看起来就和一切正常一样；但同一个故障只说一次。
        Outcome::Failure(_) => !previous.is_some_and(|run| run.status.is_failure()),
    }
}

/// 报告页的地址。实例没有配置对外地址时没有链接。
pub fn report_url(external_url: &str, run: &LogInspectionRun) -> Option<String> {
    let base = external_url.trim().trim_end_matches('/');

    (!base.is_empty()).then(|| {
        format!(
            "{base}/logs/inspections/{}?run={}",
            run.inspection_id.0, run.id.0
        )
    })
}

/// 通知的标题和正文。
pub fn message(
    pack: &LanguagePack,
    inspection: &LogInspection,
    run: &LogInspectionRun,
    timezone: Tz,
    outcome: Outcome<'_>,
    report_url: Option<&str>,
) -> Result<NotifyMessage> {
    let window = window_text(run.window_start, run.window_end, timezone);
    let url = report_url.unwrap_or_default();

    let (kind, body_key, detail_name, detail) = match outcome {
        Outcome::Report { level, summary } => {
            let summary = match summary {
                Some(summary) => summary.to_string(),
                None => pack.render("log_inspection.notify.summary.unknown", &[])?,
            };

            (level.as_str(), "report", "summary", summary)
        }
        Outcome::Failure(failure) => {
            let reason =
                pack.render(&format!("log_inspection.failure.{}", failure.as_str()), &[])?;

            ("failed", "failed", "reason", reason)
        }
    };

    let level_name = match outcome {
        Outcome::Report { level, .. } => {
            pack.render(&format!("log_inspection.level.{}", level.as_str()), &[])?
        }
        Outcome::Failure(_) => String::new(),
    };

    let fills = [
        ("name", inspection.name.as_str()),
        ("stream", inspection.stream.as_str()),
        ("window", window.as_str()),
        ("level", level_name.as_str()),
        (detail_name, detail.as_str()),
        ("report_url", url),
    ];

    let link = if report_url.is_some() {
        ""
    } else {
        "_without_link"
    };

    let mut metadata = std::collections::BTreeMap::new();
    metadata.insert("source".to_string(), "log_inspection".to_string());
    metadata.insert("inspection_id".to_string(), inspection.id.0.clone());
    metadata.insert("run_id".to_string(), run.id.0.clone());
    metadata.insert("stream".to_string(), inspection.stream.clone());
    metadata.insert("result".to_string(), kind.to_string());

    Ok(NotifyMessage {
        title: pack.render(&format!("log_inspection.notify.title.{kind}"), &fills)?,
        text: pack.render(
            &format!("log_inspection.notify.body.{body_key}{link}"),
            &fills,
        )?,
        markdown: None,
        html: None,
        metadata,
    })
}

/// 巡检覆盖的时间范围，按巡检的时区写；同一天的结束时刻不重复日期。
fn window_text(start: TimestampMicros, end: TimestampMicros, timezone: Tz) -> String {
    let start = start.to_datetime().with_timezone(&timezone);
    let end = end.to_datetime().with_timezone(&timezone);

    let end_format = if start.date_naive() == end.date_naive() {
        "%H:%M"
    } else {
        "%Y-%m-%d %H:%M"
    };

    format!(
        "{} – {} ({})",
        start.format("%Y-%m-%d %H:%M"),
        end.format(end_format),
        timezone.name()
    )
}

/// 巡检选定的通知渠道：保存时核对它能用，执行后往它发。
#[async_trait]
pub trait NotificationSender: Send + Sync {
    /// 这个渠道此刻能不能用：连接器在这个组织里，并且接受这个目标。连接器不存在时返回
    /// `Error::NotFound`。
    async fn check(&self, org_id: &Id, route: &InspectionNotifyRoute) -> Result<()>;

    async fn send(
        &self,
        org_id: &Id,
        run_id: &Id,
        route: &InspectionNotifyRoute,
        message: &NotifyMessage,
    ) -> InspectionNotification;
}

/// 生产实现：经通知连接器直接发送，投递记录里留一条（阶段 `direct`）。
pub struct ConnectorSender {
    notify: Arc<NotifyService>,
}

impl ConnectorSender {
    pub fn new(notify: Arc<NotifyService>) -> Self {
        Self { notify }
    }
}

#[async_trait]
impl NotificationSender for ConnectorSender {
    async fn check(&self, org_id: &Id, route: &InspectionNotifyRoute) -> Result<()> {
        self.notify
            .validate_direct_target(org_id, &route.connector_id, &target(route))
            .await?;

        Ok(())
    }

    async fn send(
        &self,
        org_id: &Id,
        run_id: &Id,
        route: &InspectionNotifyRoute,
        message: &NotifyMessage,
    ) -> InspectionNotification {
        let send = DirectSend {
            connector_id: route.connector_id.clone(),
            target: target(route),
            message: message.clone(),
            event_id: format!("log_inspection.run:{}", run_id.0),
            // 同一次执行对同一个渠道只发一次。
            idempotency_key: format!(
                "log_inspection:{}:{}:{}:{}",
                run_id.0,
                route.connector_id.0,
                route.target_type.as_str(),
                blake3::hash(route.target.as_bytes()).to_hex()
            ),
        };

        let outcome = self.notify.send_direct(org_id, send).await;

        InspectionNotification {
            connector_id: route.connector_id.clone(),
            target_type: route.target_type,
            target_masked: outcome.target_masked,
            delivered: outcome.delivered,
            error: outcome.error,
        }
    }
}

fn target(route: &InspectionNotifyRoute) -> NotifyTarget {
    NotifyTarget {
        target_type: route.target_type,
        value: route.target.clone(),
        metadata: Default::default(),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::{
        domain::{
            log_inspection::{InspectionSchedule, NotifyWhen, RunStatus, RunTrigger},
            notify::connector::NotifyTargetType,
        },
        shared::i18n_language_seed::seeded_pack,
    };

    pub(in crate::app::log_inspection) fn route() -> InspectionNotifyRoute {
        InspectionNotifyRoute {
            connector_id: Id::from_string("connector-1"),
            target_type: NotifyTargetType::FixedGroup,
            target: "oc_ops".into(),
        }
    }

    pub(in crate::app::log_inspection) fn inspection(when: NotifyWhen) -> LogInspection {
        LogInspection {
            id: Id::from_string("insp-1"),
            org_id: Id::from_string("org-1"),
            name: "支付日志".into(),
            stream: "app_logs".into(),
            statement: String::new(),
            window_secs: 3600,
            requirements: String::new(),
            schedule: InspectionSchedule::Interval { every_minutes: 60 },
            timezone: "Asia/Shanghai".into(),
            provider_id: Id::from_string("provider-1"),
            model: None,
            notify_when: when,
            notify_routes: vec![route()],
            language: "zh-cn".into(),
            enabled: true,
            run_as_user_id: Id::from_string("user-1"),
            next_run_at: None,
            last_run: None,
            created_by: Id::from_string("user-1"),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(0),
        }
    }

    /// 2026-10-13 08:00–09:00（上海）的一次定时执行。
    pub(in crate::app::log_inspection) fn run() -> LogInspectionRun {
        LogInspectionRun {
            id: Id::from_string("run-1"),
            inspection_id: Id::from_string("insp-1"),
            org_id: Id::from_string("org-1"),
            trigger: RunTrigger::Schedule,
            status: RunStatus::Running,
            level: None,
            window_start: TimestampMicros::from_secs(1_791_849_600),
            window_end: TimestampMicros::from_secs(1_791_853_200),
            summary: None,
            report: None,
            failure: None,
            error: None,
            provider_id: None,
            model: None,
            run_as_user_id: None,
            requested_by: None,
            tool_calls: Vec::new(),
            prompt_tokens: 0,
            completion_tokens: 0,
            notifications: Vec::new(),
            created_at: TimestampMicros::from_secs(1_791_853_200),
            started_at: Some(TimestampMicros::from_secs(1_791_853_200)),
            finished_at: None,
        }
    }

    fn last(status: RunStatus) -> LastInspectionRun {
        LastInspectionRun {
            at: TimestampMicros(1),
            status,
            level: None,
        }
    }

    fn report(level: InspectionLevel) -> Outcome<'static> {
        Outcome::Report {
            level,
            summary: None,
        }
    }

    #[test]
    fn a_report_notifies_by_the_rule_the_inspection_chose() {
        let abnormal_only = inspection(NotifyWhen::Abnormal);

        assert!(!should_notify(
            &abnormal_only,
            None,
            report(InspectionLevel::Normal)
        ));
        assert!(!should_notify(
            &abnormal_only,
            None,
            report(InspectionLevel::Attention)
        ));
        assert!(should_notify(
            &abnormal_only,
            None,
            report(InspectionLevel::Abnormal)
        ));

        // 没有判定出等级的报告，没人能说它正常。
        assert!(should_notify(
            &abnormal_only,
            None,
            report(InspectionLevel::Unknown)
        ));
        assert!(should_notify(
            &inspection(NotifyWhen::Always),
            None,
            report(InspectionLevel::Normal)
        ));
    }

    #[test]
    fn a_failure_notifies_once_when_the_inspection_goes_from_good_to_bad() {
        let inspection = inspection(NotifyWhen::Abnormal);
        let failure = Outcome::Failure(RunFailure::Model);

        // 第一次执行就失败，或上一次是成功的：通知。
        assert!(should_notify(&inspection, None, failure));
        assert!(should_notify(
            &inspection,
            Some(&last(RunStatus::Succeeded)),
            failure
        ));

        // 上一次已经失败（或超时）：同一个故障不再重复。
        assert!(!should_notify(
            &inspection,
            Some(&last(RunStatus::Failed)),
            failure
        ));
        assert!(!should_notify(
            &inspection,
            Some(&last(RunStatus::TimedOut)),
            failure
        ));
    }

    #[test]
    fn an_inspection_with_no_channel_notifies_nobody() {
        let mut silent = inspection(NotifyWhen::Always);
        silent.notify_routes.clear();

        assert!(!should_notify(
            &silent,
            None,
            report(InspectionLevel::Abnormal)
        ));
        assert!(!should_notify(
            &silent,
            None,
            Outcome::Failure(RunFailure::Agent)
        ));
    }

    #[test]
    fn an_abnormal_report_is_told_in_the_language_of_the_inspection() {
        let run = run();

        let written = message(
            &seeded_pack(LANGUAGE_KEY_PREFIX, "zh-cn"),
            &inspection(NotifyWhen::Abnormal),
            &run,
            "Asia/Shanghai".parse().unwrap(),
            Outcome::Report {
                level: InspectionLevel::Abnormal,
                summary: Some("payment-service 的错误在 08:40 后翻了三倍。"),
            },
            report_url("https://molesignal.example/", &run).as_deref(),
        )
        .unwrap();

        assert_eq!(written.title, "日志巡检发现异常：支付日志");
        assert_eq!(
            written.text,
            "数据流：app_logs\n时间范围：2026-10-13 08:00 – 09:00 (Asia/Shanghai)\n结论等级：异常\n\n\
             payment-service 的错误在 08:40 后翻了三倍。\n\n\
             查看报告：https://molesignal.example/logs/inspections/insp-1?run=run-1"
        );
        assert_eq!(written.metadata["result"], "abnormal");
        assert_eq!(written.metadata["run_id"], "run-1");
    }

    #[test]
    fn a_report_without_a_verdict_says_so_and_drops_the_link_when_there_is_none() {
        let written = message(
            &seeded_pack(LANGUAGE_KEY_PREFIX, "en-us"),
            &inspection(NotifyWhen::Abnormal),
            &run(),
            "UTC".parse().unwrap(),
            report(InspectionLevel::Unknown),
            report_url("", &run()).as_deref(),
        )
        .unwrap();

        assert_eq!(written.title, "Log inspection gave no verdict: 支付日志");
        assert_eq!(
            written.text,
            "Stream: app_logs\nPeriod: 2026-10-13 00:00 – 01:00 (UTC)\nVerdict: No verdict\n\n\
             The report does not state a verdict. Open it to read what the model wrote."
        );
    }

    #[test]
    fn a_failure_is_told_with_its_reason() {
        let run = run();

        let written = message(
            &seeded_pack(LANGUAGE_KEY_PREFIX, "zh-cn"),
            &inspection(NotifyWhen::Abnormal),
            &run,
            "Asia/Shanghai".parse().unwrap(),
            Outcome::Failure(RunFailure::TimedOut),
            report_url("https://molesignal.example", &run).as_deref(),
        )
        .unwrap();

        assert_eq!(written.title, "日志巡检没有完成：支付日志");
        assert!(written.text.contains("巡检超过了时间上限。"));
        assert!(written.text.ends_with("/logs/inspections/insp-1?run=run-1"));
        assert_eq!(written.metadata["result"], "failed");
    }

    #[test]
    fn every_level_and_failure_has_copy_in_both_languages() {
        for language in ["en-us", "zh-cn"] {
            let pack = seeded_pack(LANGUAGE_KEY_PREFIX, language);

            for level in ["normal", "attention", "abnormal", "unknown"] {
                assert!(pack.get(&format!("log_inspection.level.{level}")).is_some());
                assert!(
                    pack.get(&format!("log_inspection.notify.title.{level}"))
                        .is_some()
                );
            }

            for failure in [
                RunFailure::TimedOut,
                RunFailure::Interrupted,
                RunFailure::NotStarted,
                RunFailure::Identity,
                RunFailure::Model,
                RunFailure::Agent,
                RunFailure::Budget,
                RunFailure::Internal,
            ] {
                let key = format!("log_inspection.failure.{}", failure.as_str());
                assert!(pack.get(&key).is_some(), "{language}: {key}");
            }
        }
    }

    #[test]
    fn a_period_that_crosses_midnight_names_both_days() {
        let shanghai: Tz = "Asia/Shanghai".parse().unwrap();

        // 上海 10-13 20:00 到 10-14 08:00。
        assert_eq!(
            window_text(
                TimestampMicros::from_secs(1_791_892_800),
                TimestampMicros::from_secs(1_791_936_000),
                shanghai
            ),
            "2026-10-13 20:00 – 2026-10-14 08:00 (Asia/Shanghai)"
        );
    }
}
