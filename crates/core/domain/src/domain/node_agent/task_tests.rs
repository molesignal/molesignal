// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use kernel::{ids::Id, time::TimestampMicros};
use serde_json::json;

use super::task::{
    LeaseKey, NodeTask, RETRY_DELAY_MICROS, RejectReason, ResultOutcome, ResultReport, TaskEvent,
    TaskKind, TaskState, TaskStatus, Transition,
};

const SECOND: i64 = 1_000_000;
const TTL: i64 = 30 * SECOND;

fn at(seconds: i64) -> TimestampMicros {
    TimestampMicros(seconds * SECOND)
}

fn task() -> NodeTask {
    NodeTask {
        id: Id::from_string("task-1"),
        org_id: Id::from_string("org-a"),
        node_id: Id::from_string("node-1"),
        kind: TaskKind::Tool,
        spec: json!({"tool": "dropwatch", "duration_secs": 60}),
        state: TaskState::Queued,
        attempt: 1,
        max_attempts: 3,
        available_at: at(0),
        lease_expires_at: None,
        deadline: at(600),
        cancel_requested_at: None,
        cancel_requested_by: None,
        result: None,
        failure: None,
        created_by: "user-1".into(),
        created_at: at(0),
        updated_at: at(0),
        finished_at: None,
    }
}

fn running() -> NodeTask {
    let mut task = task();
    task.apply(
        TaskEvent::Offer {
            lease_ttl_micros: TTL,
        },
        at(1),
    );

    task.apply(
        TaskEvent::Accept {
            attempt: 1,
            lease_ttl_micros: TTL,
        },
        at(2),
    );

    task
}

fn report(attempt: u32, status: TaskStatus) -> ResultReport {
    ResultReport {
        result_id: format!("task-1/{attempt}"),
        attempt,
        status,
        summary: "events=3".into(),
        artifact_refs: vec!["pprof:sha256:ab".into()],
    }
}

#[test]
fn offer_accept_and_renew_never_shorten_the_lease_or_pass_the_deadline() {
    let mut task = task();

    assert_eq!(
        task.apply(
            TaskEvent::Offer {
                lease_ttl_micros: TTL
            },
            at(1)
        ),
        Transition::Offered {
            lease_expires_at: at(31)
        }
    );

    assert_eq!(
        task.apply(
            TaskEvent::Accept {
                attempt: 1,
                lease_ttl_micros: TTL
            },
            at(2)
        ),
        Transition::Leased {
            lease_expires_at: at(32)
        }
    );

    // A shorter TTL keeps the later expiry.
    assert_eq!(
        task.apply(
            TaskEvent::Renew {
                attempt: 1,
                lease_ttl_micros: SECOND
            },
            at(3)
        ),
        Transition::Leased {
            lease_expires_at: at(32)
        }
    );

    // A renewal never reaches past the deadline.
    assert_eq!(
        task.apply(
            TaskEvent::Renew {
                attempt: 1,
                lease_ttl_micros: 1000 * SECOND
            },
            at(20)
        ),
        Transition::Leased {
            lease_expires_at: at(600)
        }
    );

    assert_eq!(task.state, TaskState::Running);
}

#[test]
fn a_lease_of_another_attempt_or_an_expired_lease_is_stale() {
    let mut task = running();

    assert_eq!(
        task.apply(
            TaskEvent::Renew {
                attempt: 2,
                lease_ttl_micros: TTL
            },
            at(3)
        ),
        Transition::Stale
    );

    assert_eq!(
        task.apply(
            TaskEvent::Renew {
                attempt: 1,
                lease_ttl_micros: TTL
            },
            at(40)
        ),
        Transition::Stale
    );
}

#[test]
fn a_queued_task_is_offered_only_when_available_and_before_its_deadline() {
    let mut task = task();
    task.available_at = at(10);

    assert_eq!(
        task.apply(
            TaskEvent::Offer {
                lease_ttl_micros: TTL
            },
            at(5)
        ),
        Transition::Unchanged
    );

    task.available_at = at(0);

    assert_eq!(
        task.apply(
            TaskEvent::Offer {
                lease_ttl_micros: TTL
            },
            at(600)
        ),
        Transition::Unchanged
    );
}

#[test]
fn quota_and_draining_rejections_retry_as_the_next_attempt() {
    let mut task = task();
    task.apply(
        TaskEvent::Offer {
            lease_ttl_micros: TTL,
        },
        at(1),
    );

    assert_eq!(
        task.apply(
            TaskEvent::Reject {
                attempt: 1,
                reason: RejectReason::Quota
            },
            at(2)
        ),
        Transition::Requeued
    );

    assert_eq!(task.state, TaskState::Queued);
    assert_eq!(task.attempt, 2);
    assert_eq!(
        task.available_at,
        TimestampMicros(at(2).0 + RETRY_DELAY_MICROS)
    );

    // A late rejection of the first attempt changes nothing.
    assert_eq!(
        task.apply(
            TaskEvent::Reject {
                attempt: 1,
                reason: RejectReason::Draining
            },
            at(3)
        ),
        Transition::Unchanged
    );
}

#[test]
fn an_unsupported_kind_ends_the_task() {
    let mut task = task();
    task.apply(
        TaskEvent::Offer {
            lease_ttl_micros: TTL,
        },
        at(1),
    );

    assert_eq!(
        task.apply(
            TaskEvent::Reject {
                attempt: 1,
                reason: RejectReason::UnsupportedKind
            },
            at(2)
        ),
        Transition::Finished(TaskState::Rejected)
    );

    assert!(task.failure.is_some());
    assert_eq!(task.finished_at, Some(at(2)));
}

#[test]
fn a_result_is_recorded_once_and_repeats_are_duplicates() {
    let mut task = running();

    assert_eq!(
        task.apply(TaskEvent::Result(report(1, TaskStatus::Succeeded)), at(10)),
        Transition::Result(ResultOutcome::Recorded)
    );

    assert_eq!(task.state, TaskState::Succeeded);
    assert_eq!(task.finished_at, Some(at(10)));

    let duplicate = task.apply(TaskEvent::Result(report(1, TaskStatus::Failed)), at(11));
    assert_eq!(duplicate, Transition::Result(ResultOutcome::Duplicate));
    assert!(!duplicate.changed());
    assert_eq!(task.state, TaskState::Succeeded);
    assert_eq!(
        task.result.as_ref().map(|result| result.received_at),
        Some(at(10))
    );
    assert_eq!(task.updated_at, at(10));
}

#[test]
fn a_result_of_another_attempt_or_with_a_wrong_id_is_ignored() {
    let mut task = running();

    assert_eq!(
        task.apply(TaskEvent::Result(report(2, TaskStatus::Succeeded)), at(10)),
        Transition::Result(ResultOutcome::Ignored)
    );

    let mut wrong_id = report(1, TaskStatus::Succeeded);
    wrong_id.result_id = "task-2/1".into();

    assert_eq!(
        task.apply(TaskEvent::Result(wrong_id), at(10)),
        Transition::Result(ResultOutcome::Ignored)
    );

    assert!(task.result.is_none());
    assert_eq!(task.state, TaskState::Running);
}

#[test]
fn cancelling_a_queued_task_ends_it_and_a_running_one_waits_for_the_node() {
    let mut queued = task();

    assert_eq!(
        queued.apply(
            TaskEvent::Cancel {
                by: "user-1".into()
            },
            at(1)
        ),
        Transition::Finished(TaskState::Cancelled)
    );

    let mut task = running();

    assert_eq!(
        task.apply(
            TaskEvent::Cancel {
                by: "user-1".into()
            },
            at(5)
        ),
        Transition::CancelRequested
    );

    assert!(task.state.is_active());

    // A cancelled task is not renewed.
    assert_eq!(
        task.apply(
            TaskEvent::Renew {
                attempt: 1,
                lease_ttl_micros: TTL
            },
            at(6)
        ),
        Transition::Stale
    );

    assert_eq!(
        task.apply(TaskEvent::Result(report(1, TaskStatus::Cancelled)), at(7)),
        Transition::Result(ResultOutcome::Recorded)
    );

    assert_eq!(task.state, TaskState::Cancelled);

    assert_eq!(
        task.apply(
            TaskEvent::Cancel {
                by: "user-1".into()
            },
            at(8)
        ),
        Transition::Unchanged
    );
}

#[test]
fn a_cancelled_task_whose_node_never_answers_ends_with_its_lease() {
    let mut task = running();
    task.apply(
        TaskEvent::Cancel {
            by: "user-1".into(),
        },
        at(5),
    );

    assert_eq!(task.apply(TaskEvent::Expire, at(20)), Transition::Unchanged);

    assert_eq!(
        task.apply(TaskEvent::Expire, at(32)),
        Transition::Finished(TaskState::Cancelled)
    );
}

#[test]
fn expiry_retries_unaccepted_offers_and_ends_lost_leases() {
    let mut offered = task();
    offered.apply(
        TaskEvent::Offer {
            lease_ttl_micros: TTL,
        },
        at(1),
    );
    assert_eq!(
        offered.apply(TaskEvent::Expire, at(31)),
        Transition::Requeued
    );
    assert_eq!(offered.attempt, 2);

    let mut lost = running();

    assert_eq!(
        lost.apply(TaskEvent::Expire, at(32)),
        Transition::Finished(TaskState::LeaseExpired)
    );

    assert!(lost.lease_expires_at.is_none());

    let mut late = task();

    assert_eq!(
        late.apply(TaskEvent::Expire, at(600)),
        Transition::Finished(TaskState::Expired)
    );
}

#[test]
fn retries_stop_at_the_last_attempt() {
    let mut task = task();
    task.max_attempts = 2;

    for (offered_at, expected) in [
        (1, Transition::Requeued),
        (100, Transition::Finished(TaskState::Expired)),
    ] {
        task.apply(
            TaskEvent::Offer {
                lease_ttl_micros: TTL,
            },
            at(offered_at),
        );
        assert_eq!(task.apply(TaskEvent::Expire, at(offered_at + 30)), expected);
    }

    assert_eq!(task.attempt, 2);
}

#[test]
fn a_gone_node_cancels_active_tasks_only() {
    let mut task = running();

    assert_eq!(
        task.apply(
            TaskEvent::NodeGone {
                reason: "revoked".into()
            },
            at(5)
        ),
        Transition::Finished(TaskState::Cancelled)
    );

    assert_eq!(task.failure.as_deref(), Some("revoked"));

    assert_eq!(
        task.apply(
            TaskEvent::NodeGone {
                reason: "again".into()
            },
            at(6)
        ),
        Transition::Unchanged
    );
}

#[test]
fn result_ids_round_trip() {
    let key = LeaseKey {
        task_id: Id::from_string("0b6c/x"),
        attempt: 3,
    };

    assert_eq!(LeaseKey::parse_result_id(&key.result_id()), Some(key));
    assert_eq!(LeaseKey::parse_result_id("task"), None);
    assert_eq!(LeaseKey::parse_result_id("/1"), None);
}

#[test]
fn tool_specs_are_checked() {
    assert!(
        NodeTask::validate_spec(
            TaskKind::Tool,
            &json!({"tool": "perf", "duration_secs": 10})
        )
        .is_ok()
    );
    assert!(
        NodeTask::validate_spec(TaskKind::Tool, &json!({"tool": "sh", "duration_secs": 10}))
            .is_err()
    );
    assert!(
        NodeTask::validate_spec(TaskKind::Tool, &json!({"tool": "perf", "duration_secs": 0}))
            .is_err()
    );
    assert!(NodeTask::validate_spec(TaskKind::Profile, &json!([])).is_err());
    assert!(NodeTask::validate_spec(TaskKind::AutotracingRun, &json!({"rule": "cpu_sys"})).is_ok());
}
