// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{sync::Arc, time::Duration};

use super::*;
use crate::{
    app::log_inspection::{
        notification::tests::{inspection, run},
        test_support::{Agent, Channels, Identity, Model, SeededLanguage, Store},
    },
    domain::log_inspection::{NotifyWhen, RunTrigger},
    shared::ids::Id,
};

const ABNORMAL: &str =
    r#"{"level":"abnormal","summary":"payment-service 的错误翻了三倍。","evidence":[]}"#;
const NORMAL: &str = r#"{"level":"normal","summary":"没有发现异常。","evidence":[]}"#;

struct Fixture {
    store: Arc<Store>,
    agent: Arc<Agent>,
    channels: Arc<Channels>,
    runner: InspectionRunner,
}

fn fixture(inspection: LogInspection, model: Model, identity: Identity) -> Fixture {
    fixture_with(inspection, model, identity, Channels::default())
}

fn fixture_with(
    inspection: LogInspection,
    model: Model,
    identity: Identity,
    channels: Channels,
) -> Fixture {
    let store = Store::with([inspection]);
    let agent = Agent::new(model);
    let channels = Arc::new(channels);

    let runner = InspectionRunner::new(
        store.clone(),
        store.clone(),
        Arc::new(identity),
        agent.clone(),
        channels.clone(),
        Arc::new(SeededLanguage),
        "https://molesignal.example".into(),
        InspectionLimits {
            run_timeout: Duration::from_millis(50),
            ..InspectionLimits::new(3600, 20, 2, 300, 30)
        },
    );

    Fixture {
        store,
        agent,
        channels,
        runner,
    }
}

/// 一条已经开始的执行：记录在库里，状态是执行中。
fn started(fixture: &Fixture) -> LogInspectionRun {
    let run = run();
    fixture.store.put_run(run.clone());
    run
}

#[tokio::test]
async fn an_abnormal_report_is_saved_with_its_level_and_notified() {
    let inspection = inspection(NotifyWhen::Abnormal);

    let fixture = fixture(
        inspection.clone(),
        Model::Reports(
            ABNORMAL,
            &[("get_stream_schema", true), ("query_logs", true)],
        ),
        Identity::Usable,
    );

    let run = started(&fixture);
    let outcome = fixture.runner.execute(&inspection, &run).await;

    assert_eq!(
        outcome,
        RunOutcome {
            status: RunStatus::Succeeded,
            level: Some(InspectionLevel::Abnormal),
            notifications: 1,
        }
    );

    let saved = fixture.store.run(&run.id);
    assert_eq!(saved.status, RunStatus::Succeeded);
    assert_eq!(saved.level, Some(InspectionLevel::Abnormal));
    assert_eq!(
        saved.summary.as_deref(),
        Some("payment-service 的错误翻了三倍。")
    );
    assert_eq!(saved.report.as_deref(), Some(ABNORMAL));
    assert_eq!(saved.model.as_deref(), Some("default-model"));
    assert_eq!((saved.prompt_tokens, saved.completion_tokens), (1_200, 300));
    assert_eq!(saved.run_as_user_id, Some(Id::from_string("user-1")));
    assert!(saved.finished_at.is_some());

    // 只记工具的名字和成败。
    let tools = saved
        .tool_calls
        .iter()
        .map(|call| (call.name.as_str(), call.ok))
        .collect::<Vec<_>>();
    assert_eq!(tools, [("get_stream_schema", true), ("query_logs", true)]);

    // 巡检的「最近一次执行」跟着变。
    let last = fixture.store.inspection("insp-1").last_run.unwrap();
    assert_eq!(last.status, RunStatus::Succeeded);
    assert_eq!(last.level, Some(InspectionLevel::Abnormal));

    let sent = fixture.channels.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, run.id);
    assert_eq!(sent[0].2.title, "日志巡检发现异常：支付日志");
    assert!(sent[0].2.text.contains("payment-service 的错误翻了三倍。"));
    assert!(saved.notifications[0].delivered);
}

#[tokio::test]
async fn a_normal_report_is_saved_and_nobody_is_told() {
    let inspection = inspection(NotifyWhen::Abnormal);
    let fixture = fixture(
        inspection.clone(),
        Model::Reports(NORMAL, &[]),
        Identity::Usable,
    );
    let run = started(&fixture);

    let outcome = fixture.runner.execute(&inspection, &run).await;

    assert_eq!(outcome.level, Some(InspectionLevel::Normal));
    assert_eq!(outcome.notifications, 0);
    assert!(fixture.channels.sent().is_empty());
    assert!(fixture.store.run(&run.id).notifications.is_empty());
}

#[tokio::test]
async fn a_report_that_states_no_level_is_kept_and_notified_as_having_no_verdict() {
    let inspection = inspection(NotifyWhen::Abnormal);

    let fixture = fixture(
        inspection.clone(),
        Model::Reports("日志整体平稳，没有需要处理的问题。", &[]),
        Identity::Usable,
    );

    let run = started(&fixture);
    let outcome = fixture.runner.execute(&inspection, &run).await;

    assert_eq!(outcome.status, RunStatus::Succeeded);
    assert_eq!(outcome.level, Some(InspectionLevel::Unknown));

    // 模型写了什么照样留着，只是没有等级。
    let saved = fixture.store.run(&run.id);
    assert_eq!(
        saved.report.as_deref(),
        Some("日志整体平稳，没有需要处理的问题。")
    );
    assert_eq!(saved.summary, None);
    assert_eq!(
        fixture.channels.sent()[0].2.title,
        "日志巡检没有给出结论等级：支付日志"
    );
}

#[tokio::test]
async fn the_model_is_asked_about_the_stream_and_period_of_this_run() {
    let mut inspection = inspection(NotifyWhen::Abnormal);
    inspection.statement = "level = 'ERROR'".into();
    let fixture = fixture(
        inspection.clone(),
        Model::Reports(NORMAL, &[]),
        Identity::Usable,
    );
    let run = started(&fixture);

    fixture.runner.execute(&inspection, &run).await;

    let requests = fixture.agent.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].0, "app_logs");
    assert!(requests[0].1.contains("level = 'ERROR'"));
    assert!(requests[0].1.contains("\"start_micros\": 1791849600000000"));
}

#[tokio::test]
async fn a_run_that_outlives_the_time_limit_is_cut_and_recorded_as_timed_out() {
    let inspection = inspection(NotifyWhen::Abnormal);
    let fixture = fixture(inspection.clone(), Model::Hangs, Identity::Usable);
    let run = started(&fixture);

    let outcome = fixture.runner.execute(&inspection, &run).await;

    assert_eq!(outcome.status, RunStatus::TimedOut);
    assert_eq!(outcome.level, None);

    let saved = fixture.store.run(&run.id);
    assert_eq!(saved.status, RunStatus::TimedOut);
    assert_eq!(saved.failure, Some(RunFailure::TimedOut));
    assert!(saved.report.is_none());

    // 超时之前调用过的工具留得下来。
    assert_eq!(saved.tool_calls.len(), 1);
    assert_eq!(saved.tool_calls[0].name, "query_logs");

    // 第一次执行就没完成：通知一次。
    let sent = fixture.channels.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].2.title, "日志巡检没有完成：支付日志");
    assert!(sent[0].2.text.contains("巡检超过了时间上限。"));
}

#[tokio::test]
async fn a_saver_who_can_no_longer_be_used_fails_the_run_without_calling_the_model() {
    let inspection = inspection(NotifyWhen::Abnormal);

    let fixture = fixture(
        inspection.clone(),
        Model::Reports(NORMAL, &[]),
        Identity::Unusable("the inspection runs as a@example.com, who no longer holds agent.use"),
    );

    let run = started(&fixture);
    let outcome = fixture.runner.execute(&inspection, &run).await;

    assert_eq!(outcome.status, RunStatus::Failed);
    assert!(fixture.agent.requests().is_empty());

    let saved = fixture.store.run(&run.id);
    assert_eq!(saved.failure, Some(RunFailure::Identity));
    assert!(saved.error.unwrap().contains("no longer holds agent.use"));
    assert!(
        fixture.channels.sent()[0]
            .2
            .text
            .contains("巡检的执行身份已不可用")
    );
}

#[tokio::test]
async fn each_way_of_failing_is_recorded_with_its_own_reason() {
    for (model, identity, failure) in [
        (Model::Unusable, Identity::Usable, RunFailure::Model),
        (Model::Fails, Identity::Usable, RunFailure::Agent),
        (
            Model::Reports(NORMAL, &[]),
            Identity::Unreachable,
            RunFailure::Internal,
        ),
    ] {
        let inspection = inspection(NotifyWhen::Abnormal);
        let fixture = fixture(inspection.clone(), model, identity);
        let run = started(&fixture);

        let outcome = fixture.runner.execute(&inspection, &run).await;

        assert_eq!(outcome.status, RunStatus::Failed, "{failure:?}");
        assert_eq!(fixture.store.run(&run.id).failure, Some(failure));
    }
}

#[tokio::test]
async fn a_failure_that_repeats_is_told_once() {
    let fixture = fixture(
        inspection(NotifyWhen::Abnormal),
        Model::Fails,
        Identity::Usable,
    );

    // 第一次失败：由好转坏，通知。
    let first = started(&fixture);
    let before = fixture.store.inspection("insp-1");
    assert_eq!(
        fixture.runner.execute(&before, &first).await.notifications,
        1
    );

    // 第二次还是失败：巡检上一次的状况已经是失败，不再重复。
    let second = LogInspectionRun {
        id: Id::from_string("run-2"),
        ..run()
    };
    fixture.store.put_run(second.clone());
    let before = fixture.store.inspection("insp-1");
    assert_eq!(
        fixture.runner.execute(&before, &second).await.notifications,
        0
    );
    assert_eq!(fixture.channels.sent().len(), 1);
}

#[tokio::test]
async fn a_channel_that_does_not_deliver_is_recorded_on_the_run() {
    let inspection = inspection(NotifyWhen::Always);

    let fixture = fixture_with(
        inspection.clone(),
        Model::Reports(NORMAL, &[]),
        Identity::Usable,
        Channels {
            undelivered: true,
            ..Channels::default()
        },
    );

    let run = started(&fixture);
    let outcome = fixture.runner.execute(&inspection, &run).await;

    // 报告照样保存，通知的结果如实记下。
    assert_eq!(outcome.status, RunStatus::Succeeded);
    assert_eq!(outcome.notifications, 1);

    let recorded = fixture.store.run(&run.id).notifications;
    assert!(!recorded[0].delivered);
    assert_eq!(
        recorded[0].error.as_deref(),
        Some("webhook returned HTTP 502")
    );
}

#[tokio::test]
async fn a_run_already_ended_elsewhere_is_left_alone() {
    let inspection = inspection(NotifyWhen::Always);
    let fixture = fixture(
        inspection.clone(),
        Model::Reports(ABNORMAL, &[]),
        Identity::Usable,
    );

    // 记录不在库里的「执行中」：等同于已经被判为中断。
    let outcome = fixture.runner.execute(&inspection, &run()).await;

    assert_eq!(outcome.notifications, 0);
    assert!(fixture.channels.sent().is_empty());
    assert!(fixture.store.inspection("insp-1").last_run.is_none());
}

#[tokio::test]
async fn a_due_inspection_begins_a_run_over_its_window_and_moves_to_the_next_period() {
    let now = TimestampMicros::from_secs(1_791_853_200);

    let due = LogInspection {
        next_run_at: Some(TimestampMicros(now.0 - 1)),
        ..inspection(NotifyWhen::Abnormal)
    };

    let fixture = fixture(due, Model::Reports(NORMAL, &[]), Identity::Usable);

    let (inspection, run) = fixture
        .runner
        .begin_scheduled(&Id::from_string("insp-1"), now)
        .await
        .unwrap()
        .expect("a due inspection begins a run");

    assert_eq!(run.trigger, RunTrigger::Schedule);
    assert_eq!(run.status, RunStatus::Running);
    assert_eq!(run.window_end, now);
    assert_eq!(run.window_start, TimestampMicros(now.0 - 3_600_000_000));
    assert_eq!(inspection.id.0, "insp-1");

    // 每小时一次：下一次是一小时后，同一拍里不会再到期。
    let stored = fixture.store.inspection("insp-1");
    assert_eq!(
        stored.next_run_at,
        Some(TimestampMicros(now.0 + 3_600_000_000))
    );

    let again = fixture
        .runner
        .begin_scheduled(&Id::from_string("insp-1"), now)
        .await
        .unwrap();
    assert!(again.is_none());
}

#[tokio::test]
async fn an_inspection_with_a_run_in_progress_does_not_begin_another() {
    let now = TimestampMicros::from_secs(1_791_853_200);

    let due = LogInspection {
        next_run_at: Some(now),
        ..inspection(NotifyWhen::Abnormal)
    };

    let fixture = fixture(due, Model::Reports(NORMAL, &[]), Identity::Usable);
    started(&fixture);

    let begun = fixture
        .runner
        .begin_scheduled(&Id::from_string("insp-1"), now)
        .await
        .unwrap();

    // 没有开始，也没有把这次周期跳过去：下一拍再看。
    assert!(begun.is_none());
    assert_eq!(fixture.store.inspection("insp-1").next_run_at, Some(now));
}

#[tokio::test]
async fn a_disabled_or_deleted_inspection_does_not_begin_a_run() {
    let now = TimestampMicros::from_secs(1_791_853_200);

    let disabled = LogInspection {
        enabled: false,
        next_run_at: Some(now),
        ..inspection(NotifyWhen::Abnormal)
    };

    let fixture = fixture(disabled, Model::Reports(NORMAL, &[]), Identity::Usable);
    let runner = &fixture.runner;

    assert!(
        runner
            .begin_scheduled(&Id::from_string("insp-1"), now)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        runner
            .begin_scheduled(&Id::from_string("gone"), now)
            .await
            .unwrap()
            .is_none()
    );
    assert!(fixture.store.runs().is_empty());
}

#[tokio::test]
async fn a_queued_run_is_taken_once_and_covers_the_time_it_starts_at() {
    let fixture = fixture(
        inspection(NotifyWhen::Abnormal),
        Model::Reports(NORMAL, &[]),
        Identity::Usable,
    );

    let queued = LogInspectionRun {
        trigger: RunTrigger::Manual,
        status: RunStatus::Queued,
        started_at: None,
        requested_by: Some(Id::from_string("user-2")),
        ..run()
    };
    fixture.store.put_run(queued.clone());

    // 点下按钮两分钟后才轮到它。
    let now = TimestampMicros(queued.created_at.0 + 120_000_000);

    let (_, taken) = fixture
        .runner
        .begin_queued(&queued, now)
        .await
        .unwrap()
        .expect("a queued run is taken");

    assert_eq!(taken.status, RunStatus::Running);
    assert_eq!(taken.window_end, now);
    assert_eq!(taken.window_start, TimestampMicros(now.0 - 3_600_000_000));
    assert_eq!(taken.requested_by, Some(Id::from_string("user-2")));
    assert_eq!(fixture.store.run(&queued.id).status, RunStatus::Running);

    // 另一个节点晚一步：领不到。
    assert!(
        fixture
            .runner
            .begin_queued(&queued, now)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn runs_left_behind_are_found_and_recorded_as_failed() {
    let fixture = fixture(
        inspection(NotifyWhen::Abnormal),
        Model::Reports(NORMAL, &[]),
        Identity::Usable,
    );

    let interrupted = started(&fixture);
    let started_at = interrupted.started_at.unwrap();

    // 总超时（这里是 50 毫秒）加两分钟之内，还可能只是慢。
    let soon = TimestampMicros(started_at.0 + 60_000_000);
    assert!(fixture.runner.stale(soon).await.unwrap().is_empty());

    let later = TimestampMicros(started_at.0 + 180_000_000);
    let stale = fixture.runner.stale(later).await.unwrap();
    assert_eq!(stale.len(), 1);

    let outcome = fixture.runner.abandon(&stale[0]).await.unwrap().unwrap();
    assert_eq!(outcome.status, RunStatus::Failed);
    assert_eq!(outcome.notifications, 1);

    let saved = fixture.store.run(&interrupted.id);
    assert_eq!(saved.failure, Some(RunFailure::Interrupted));
    assert_eq!(
        fixture.store.inspection("insp-1").last_run.unwrap().status,
        RunStatus::Failed
    );

    // 已经了结的记录不会被了结第二次，也不会再通知。
    assert!(fixture.runner.stale(later).await.unwrap().is_empty());
    assert_eq!(
        fixture
            .runner
            .abandon(&stale[0])
            .await
            .unwrap()
            .unwrap()
            .notifications,
        0
    );
    assert_eq!(fixture.channels.sent().len(), 1);
}

#[tokio::test]
async fn a_queued_run_nobody_picks_up_is_recorded_as_not_started() {
    let fixture = fixture(
        inspection(NotifyWhen::Abnormal),
        Model::Reports(NORMAL, &[]),
        Identity::Usable,
    );

    let queued = LogInspectionRun {
        trigger: RunTrigger::Manual,
        status: RunStatus::Queued,
        started_at: None,
        ..run()
    };
    fixture.store.put_run(queued.clone());

    let waited = TimestampMicros(queued.created_at.0 + 16 * 60 * 1_000_000);
    let stale = fixture.runner.stale(waited).await.unwrap();
    assert_eq!(stale.len(), 1);

    fixture.runner.abandon(&stale[0]).await.unwrap();

    let saved = fixture.store.run(&queued.id);
    assert_eq!(saved.status, RunStatus::Failed);
    assert_eq!(saved.failure, Some(RunFailure::NotStarted));
    assert!(saved.started_at.is_none());
}

#[tokio::test]
async fn reports_past_the_retention_period_are_removed() {
    let fixture = fixture(
        inspection(NotifyWhen::Abnormal),
        Model::Reports(NORMAL, &[]),
        Identity::Usable,
    );

    let day = 24 * 3600 * 1_000_000_i64;
    let now = TimestampMicros(100 * day);

    for (id, age_days, status) in [
        ("old", 31, RunStatus::Succeeded),
        ("recent", 29, RunStatus::Succeeded),
        ("old-but-running", 31, RunStatus::Running),
    ] {
        fixture.store.put_run(LogInspectionRun {
            id: Id::from_string(id),
            inspection_id: Id::from_string(format!("insp-{id}")),
            status,
            created_at: TimestampMicros(now.0 - age_days * day),
            ..run()
        });
    }

    assert_eq!(fixture.runner.purge_expired(now).await.unwrap(), 1);

    let kept = fixture
        .store
        .runs()
        .into_iter()
        .map(|run| run.id.0)
        .collect::<Vec<_>>();
    assert_eq!(kept, ["recent", "old-but-running"]);
}
