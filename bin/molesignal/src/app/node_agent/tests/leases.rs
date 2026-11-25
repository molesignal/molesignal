// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Leases, results and quota: expiry, duplicates, reconnect reconciliation, cancellation and the
//! slots a finished task gives back.

use domain::node_agent::{Health, TaskQuota, TaskState, TaskStatus};
use kernel::{Error, time::TimestampMicros};

use super::{Fixture, SECOND, acks, cancels, hello, member, offers, org, result, run};
use crate::app::node_agent::{HeartbeatInput, Outbound};

fn heartbeat(seq: u64, leases: Vec<domain::node_agent::LeaseKey>) -> HeartbeatInput {
    HeartbeatInput {
        seq,
        applied_config_revision: 0,
        health: Health::default(),
        collector_states: Vec::new(),
        active_leases: leases,
    }
}

fn later(seconds: i64) -> TimestampMicros {
    TimestampMicros(TimestampMicros::now().0 + seconds * SECOND)
}

#[tokio::test]
async fn heartbeats_renew_a_live_lease_and_cancel_an_expired_one() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;
    let task = fixture.task(&org_a, &node).await.unwrap();
    let (mut session, out) = fixture.open(&node).await;
    let key = run(&fixture, &mut session, &out).await;
    assert_eq!(key, task.lease_key());

    let out = fixture
        .service
        .heartbeat(&mut session, heartbeat(1, vec![key.clone()]))
        .await
        .unwrap();

    let Outbound::HeartbeatAck { seq, renewals, .. } = &out[0] else {
        panic!("{out:?}");
    };

    assert_eq!(*seq, 1);
    assert_eq!(renewals.len(), 1);
    assert!(renewals[0].1 <= task.deadline);

    // The node goes quiet past its lease.
    assert_eq!(fixture.service.sweep_expired(later(130)).await.unwrap(), 1);

    let task = fixture.service.get_task(&org_a, &task.id).await.unwrap();
    assert_eq!(task.state, TaskState::LeaseExpired);
    assert!(task.finished_at.is_some());

    let out = fixture
        .service
        .heartbeat(&mut session, heartbeat(2, vec![key.clone()]))
        .await
        .unwrap();
    assert!(matches!(&out[0], Outbound::HeartbeatAck { renewals, .. } if renewals.is_empty()));
    assert_eq!(cancels(&out), [key]);
}

#[tokio::test]
async fn an_unaccepted_offer_is_offered_again_as_the_next_attempt() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;
    let task = fixture.task(&org_a, &node).await.unwrap();
    let (_, out) = fixture.open(&node).await;
    assert_eq!(offers(&out), [task.lease_key()]);

    fixture.service.sweep_expired(later(61)).await.unwrap();
    let task = fixture.service.get_task(&org_a, &task.id).await.unwrap();
    assert_eq!((task.state, task.attempt), (TaskState::Queued, 2));
}

#[tokio::test]
async fn a_result_is_recorded_once_and_acknowledged_every_time() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;
    let task = fixture.task(&org_a, &node).await.unwrap();
    let (mut session, out) = fixture.open(&node).await;
    let key = run(&fixture, &mut session, &out).await;

    let out = fixture
        .service
        .task_result(&session, result(&key, TaskStatus::Succeeded))
        .await
        .unwrap();
    assert_eq!(acks(&out), [key.result_id()]);
    let recorded = fixture.service.get_task(&org_a, &task.id).await.unwrap();
    assert_eq!(recorded.state, TaskState::Succeeded);

    // The same result again (a lost ack), even with another status, changes nothing.
    let out = fixture
        .service
        .task_result(&session, result(&key, TaskStatus::Failed))
        .await
        .unwrap();
    assert_eq!(acks(&out), [key.result_id()]);
    let again = fixture.service.get_task(&org_a, &task.id).await.unwrap();
    assert_eq!(again.state, TaskState::Succeeded);
    assert_eq!(again.result, recorded.result);
    assert_eq!(again.updated_at, recorded.updated_at);

    // After a reconnect, a result still pending on the node is acknowledged in reply to Hello.
    let (_, out) = fixture
        .connect(&node, hello(&node, Vec::new(), vec![key.result_id()]))
        .await;
    assert_eq!(acks(&out), [key.result_id()]);
}

#[tokio::test]
async fn a_reconnect_cancels_leases_the_control_plane_no_longer_holds() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;
    let kept = fixture.task(&org_a, &node).await.unwrap();
    let dropped = fixture.task(&org_a, &node).await.unwrap();
    let (mut session, _) = fixture.open(&node).await;
    let out = fixture.service.poll(&mut session).await.unwrap();
    assert!(offers(&out).is_empty(), "both were offered at Hello");

    for key in [kept.lease_key(), dropped.lease_key()] {
        fixture.service.lease_accept(&session, key).await.unwrap();
    }

    fixture
        .service
        .cancel_task(&org_a, &member(), &dropped.id)
        .await
        .unwrap();

    let (_, out) = fixture
        .connect(
            &node,
            hello(
                &node,
                vec![kept.lease_key(), dropped.lease_key()],
                Vec::new(),
            ),
        )
        .await;

    assert_eq!(cancels(&out), [dropped.lease_key()]);
}

#[tokio::test]
async fn cancelling_a_running_task_tells_the_node_once_and_ends_with_its_result() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;
    let task = fixture.task(&org_a, &node).await.unwrap();
    let (mut session, out) = fixture.open(&node).await;
    let key = run(&fixture, &mut session, &out).await;

    let cancelled = fixture
        .service
        .cancel_task(&org_a, &member(), &task.id)
        .await
        .unwrap();
    assert_eq!(cancelled.state, TaskState::Running);
    assert!(cancelled.cancel_requested_at.is_some());

    assert_eq!(
        cancels(&fixture.service.poll(&mut session).await.unwrap()),
        std::slice::from_ref(&key)
    );
    assert!(cancels(&fixture.service.poll(&mut session).await.unwrap()).is_empty());

    fixture
        .service
        .task_result(&session, result(&key, TaskStatus::Cancelled))
        .await
        .unwrap();
    let task = fixture.service.get_task(&org_a, &task.id).await.unwrap();
    assert_eq!(task.state, TaskState::Cancelled);
    assert!(
        fixture
            .audit
            .actions(&org_a)
            .contains(&"node_agent.task.cancel".to_string())
    );
}

#[tokio::test]
async fn a_queued_task_past_its_deadline_expires() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;
    let task = fixture.task(&org_a, &node).await.unwrap();

    fixture.service.sweep_expired(later(601)).await.unwrap();
    let task = fixture.service.get_task(&org_a, &task.id).await.unwrap();
    assert_eq!(task.state, TaskState::Expired);
    assert!(task.failure.is_some());
}

#[tokio::test]
async fn a_finished_or_cancelled_task_gives_its_quota_slot_back() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;

    fixture
        .service
        .set_task_quota(
            &org_a,
            &member(),
            TaskQuota {
                max_active_per_org: 1,
                max_active_per_node: 1,
                max_created_per_hour_per_org: 100,
                max_created_per_hour_per_node: 100,
            },
        )
        .await
        .unwrap();

    let first = fixture.task(&org_a, &node).await.unwrap();
    let refused = fixture.task(&org_a, &node).await.unwrap_err();
    assert!(
        matches!(refused, Error::ResourceExhausted(_)),
        "{refused:?}"
    );

    // The result frees the slot.
    let (mut session, out) = fixture.open(&node).await;
    let key = run(&fixture, &mut session, &out).await;
    assert_eq!(key, first.lease_key());
    fixture
        .service
        .task_result(&session, result(&key, TaskStatus::Succeeded))
        .await
        .unwrap();
    let second = fixture.task(&org_a, &node).await.unwrap();

    // Cancelling a queued task frees it too.
    fixture
        .service
        .cancel_task(&org_a, &member(), &second.id)
        .await
        .unwrap();
    let third = fixture.task(&org_a, &node).await.unwrap();

    // So does a lease that runs out.
    let key = run(&fixture, &mut session, &[]).await;
    assert_eq!(key, third.lease_key());
    assert!(fixture.task(&org_a, &node).await.is_err());
    fixture.service.sweep_expired(later(130)).await.unwrap();
    assert!(fixture.task(&org_a, &node).await.is_ok());

    assert!(
        fixture
            .audit
            .actions(&org_a)
            .contains(&"node_agent.quota.update".to_string())
    );
}

#[tokio::test]
async fn the_hourly_limit_holds_even_when_tasks_finish() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;

    fixture
        .service
        .set_task_quota(
            &org_a,
            &member(),
            TaskQuota {
                max_active_per_org: 10,
                max_active_per_node: 10,
                max_created_per_hour_per_org: 10,
                max_created_per_hour_per_node: 2,
            },
        )
        .await
        .unwrap();

    for _ in 0..2 {
        let task = fixture.task(&org_a, &node).await.unwrap();
        fixture
            .service
            .cancel_task(&org_a, &member(), &task.id)
            .await
            .unwrap();
    }

    let refused = fixture.task(&org_a, &node).await.unwrap_err();
    assert!(
        matches!(refused, Error::ResourceExhausted(_)),
        "{refused:?}"
    );
}

#[tokio::test]
async fn offers_stop_at_the_smaller_of_the_node_quota_and_the_agent_capacity() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;

    for _ in 0..4 {
        fixture.task(&org_a, &node).await.unwrap();
    }

    let mut hello = hello(&node, Vec::new(), Vec::new());
    hello.capabilities.max_running_tasks = 2;
    let (session, out) = fixture.connect(&node, hello).await;
    assert_eq!(offers(&out).len(), 2);
    assert!(offers(&fixture.service.poll(&mut session.unwrap()).await.unwrap()).is_empty());
}
