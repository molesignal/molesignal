// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::time::Duration;

use async_trait::async_trait;

use super::*;
use crate::{
    app::log_inspection::{
        InspectionLimits,
        test_support::{Agent, Channels, Identity, Model, SeededLanguage, Store},
    },
    domain::log_inspection::{
        InspectionLevel, InspectionSchedule, NotifyWhen, RunStatus, RunTrigger,
    },
    shared::ids::Id,
};

const NORMAL: &str = r#"{"level":"normal","summary":"quiet","evidence":[]}"#;
const NOW: TimestampMicros = TimestampMicros(1_791_853_200_000_000);

/// 一条每小时一次、已经到期的巡检。
fn due(id: &str) -> LogInspection {
    LogInspection {
        id: Id::from_string(id),
        org_id: Id::from_string("org-1"),
        name: id.into(),
        stream: "app_logs".into(),
        statement: String::new(),
        window_secs: 3600,
        requirements: String::new(),
        schedule: InspectionSchedule::Interval { every_minutes: 60 },
        timezone: "UTC".into(),
        provider_id: Id::from_string("provider-1"),
        model: None,
        notify_when: NotifyWhen::Abnormal,
        notify_routes: Vec::new(),
        language: "en-us".into(),
        enabled: true,
        run_as_user_id: Id::from_string("user-1"),
        next_run_at: Some(TimestampMicros(NOW.0 - 1)),
        last_run: None,
        created_by: Id::from_string("user-1"),
        created_at: TimestampMicros(0),
        updated_at: TimestampMicros(0),
    }
}

struct Fixture {
    store: Arc<Store>,
    agent: Arc<Agent>,
    runner: Arc<InspectionRunner>,
}

fn fixture(inspections: Vec<LogInspection>, model: Model, concurrent_runs: usize) -> Fixture {
    let store = Store::with(inspections);
    let agent = Agent::new(model);

    let runner = Arc::new(InspectionRunner::new(
        store.clone(),
        store.clone(),
        Arc::new(Identity::Usable),
        agent.clone(),
        Arc::new(Channels::default()),
        Arc::new(SeededLanguage),
        String::new(),
        InspectionLimits {
            concurrent_runs,
            run_timeout: Duration::from_millis(200),
            ..InspectionLimits::new(3600, 20, 2, 300, 30)
        },
    ));

    Fixture {
        store,
        agent,
        runner,
    }
}

async fn finished(started: Vec<JoinHandle<()>>) {
    for run in started {
        run.await.expect("the run task ends");
    }
}

/// 所有巡检都归别的节点。
struct OwnedElsewhere;

#[async_trait]
impl RunGate for OwnedElsewhere {
    async fn admit(&self, _kind: &'static str, _key: &str) -> Result<Option<RunPermit>> {
        Ok(None)
    }
}

#[tokio::test]
async fn a_due_inspection_is_run_once_and_waits_for_its_next_period() {
    let fixture = fixture(vec![due("insp-1")], Model::Reports(NORMAL, &[]), 2);
    let worker = LogInspectionWorker::new(fixture.runner.clone());

    let started = worker.tick_once(NOW).await.unwrap();
    assert_eq!(started.len(), 1);
    finished(started).await;

    let runs = fixture.store.runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].trigger, RunTrigger::Schedule);
    assert_eq!(runs[0].status, RunStatus::Succeeded);
    assert_eq!(runs[0].level, Some(InspectionLevel::Normal));
    assert_eq!(fixture.agent.requests().len(), 1);

    // 同一个时刻再来一拍：下次执行时间已经推到一小时后，不会再跑。
    assert!(worker.tick_once(NOW).await.unwrap().is_empty());

    // 一小时后到期，再执行一次。
    let later = TimestampMicros(NOW.0 + 3_600_000_000);
    finished(worker.tick_once(later).await.unwrap()).await;
    assert_eq!(fixture.store.runs().len(), 2);
}

#[tokio::test]
async fn an_inspection_owned_by_another_node_is_left_alone() {
    let fixture = fixture(vec![due("insp-1")], Model::Reports(NORMAL, &[]), 2);

    let worker =
        LogInspectionWorker::new(fixture.runner.clone()).with_gate(Arc::new(OwnedElsewhere));

    assert!(worker.tick_once(NOW).await.unwrap().is_empty());
    assert!(fixture.store.runs().is_empty());

    // 没轮到本节点，也没有把这次周期跳过去。
    assert_eq!(
        fixture.store.inspection("insp-1").next_run_at,
        Some(TimestampMicros(NOW.0 - 1))
    );
}

#[tokio::test]
async fn a_node_runs_no_more_inspections_at_once_than_it_is_allowed() {
    let fixture = fixture(
        vec![due("insp-1"), due("insp-2"), due("insp-3")],
        Model::Hangs,
        2,
    );

    let worker = LogInspectionWorker::new(fixture.runner.clone());

    // 两个名额：第三条留到下一拍。
    let first = worker.tick_once(NOW).await.unwrap();
    assert_eq!(first.len(), 2);
    assert!(worker.tick_once(NOW).await.unwrap().is_empty());

    // 前两条超时结束、让出名额后，第三条开始。
    finished(first).await;
    let second = worker.tick_once(NOW).await.unwrap();
    assert_eq!(second.len(), 1);
    finished(second).await;

    let runs = fixture.store.runs();
    assert_eq!(runs.len(), 3);
    assert!(runs.iter().all(|run| run.status == RunStatus::TimedOut));
}

#[tokio::test]
async fn a_manual_run_is_picked_up_even_when_the_inspection_is_switched_off() {
    let paused = LogInspection {
        enabled: false,
        next_run_at: None,
        ..due("insp-1")
    };

    let fixture = fixture(vec![paused.clone()], Model::Reports(NORMAL, &[]), 2);
    let worker = LogInspectionWorker::new(fixture.runner.clone());

    // 没有排队的手动触发时，停用的巡检不执行。
    assert!(worker.tick_once(NOW).await.unwrap().is_empty());

    fixture.store.put_run(LogInspectionRun {
        id: Id::from_string("run-manual"),
        inspection_id: paused.id.clone(),
        org_id: paused.org_id.clone(),
        trigger: RunTrigger::Manual,
        status: RunStatus::Queued,
        level: None,
        window_start: NOW,
        window_end: NOW,
        summary: None,
        report: None,
        failure: None,
        error: None,
        provider_id: None,
        model: None,
        run_as_user_id: None,
        requested_by: Some(Id::from_string("user-2")),
        tool_calls: Vec::new(),
        prompt_tokens: 0,
        completion_tokens: 0,
        notifications: Vec::new(),
        created_at: NOW,
        started_at: None,
        finished_at: None,
    });

    finished(worker.tick_once(NOW).await.unwrap()).await;

    let run = fixture.store.run(&Id::from_string("run-manual"));
    assert_eq!(run.status, RunStatus::Succeeded);
    assert_eq!(run.window_end, NOW);

    // 手动执行不改变它的启停，也不给它排上周期。
    let inspection = fixture.store.inspection("insp-1");
    assert!(!inspection.enabled);
    assert_eq!(inspection.next_run_at, None);
}

#[tokio::test]
async fn a_run_left_by_a_node_that_stopped_is_settled_on_a_later_tick() {
    let inspection = LogInspection {
        next_run_at: Some(TimestampMicros(NOW.0 + 3_600_000_000)),
        ..due("insp-1")
    };

    let fixture = fixture(vec![inspection.clone()], Model::Reports(NORMAL, &[]), 2);
    let worker = LogInspectionWorker::new(fixture.runner.clone());

    // 十分钟前开始、至今还标着执行中：执行它的节点已经不在了。
    let started_at = TimestampMicros(NOW.0 - 600_000_000);

    fixture.store.put_run(LogInspectionRun {
        id: Id::from_string("run-lost"),
        inspection_id: inspection.id.clone(),
        org_id: inspection.org_id.clone(),
        trigger: RunTrigger::Schedule,
        status: RunStatus::Running,
        level: None,
        window_start: started_at,
        window_end: started_at,
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
        created_at: started_at,
        started_at: Some(started_at),
        finished_at: None,
    });

    assert!(worker.tick_once(NOW).await.unwrap().is_empty());

    let run = fixture.store.run(&Id::from_string("run-lost"));
    assert_eq!(run.status, RunStatus::Failed);

    assert_eq!(
        run.failure,
        Some(crate::domain::log_inspection::RunFailure::Interrupted)
    );
    assert!(fixture.agent.requests().is_empty());
}
