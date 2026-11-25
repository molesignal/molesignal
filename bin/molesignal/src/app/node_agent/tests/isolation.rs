// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Nothing of one organization is reachable from another: not over the management calls, and not
//! from a node session.

use domain::node_agent::{LeaseKey, TaskFilter, TaskState, TaskStatus};
use kernel::Error;
use serde_json::json;

use super::{Fixture, acks, cancels, member, offers, org, result};

#[tokio::test]
async fn management_calls_never_reach_another_organizations_nodes_or_tasks() {
    let fixture = Fixture::new();
    let (org_a, org_b) = (org("org-a"), org("org-b"));
    let node_a = fixture.enroll(&org_a, "shared-host").await;
    let node_b = fixture.enroll(&org_b, "shared-host").await;
    let task_a = fixture.task(&org_a, &node_a).await.unwrap();

    // The same host enrolled in two organizations is two nodes.
    assert_ne!(node_a.id(), node_b.id());

    let listed = fixture.service.list_nodes(&org_b).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(&listed[0].node.id, node_b.id());
    assert!(
        fixture
            .service
            .list_tasks(&org_b, TaskFilter::default())
            .await
            .unwrap()
            .is_empty()
    );

    let refusals = [
        fixture.service.get_node(&org_b, node_a.id()).await.err(),
        fixture
            .service
            .drain(&org_b, &member(), node_a.id(), 60)
            .await
            .err(),
        fixture
            .service
            .revoke(&org_b, &member(), node_a.id(), "nope")
            .await
            .err(),
        fixture
            .service
            .set_config(&org_b, &member(), node_a.id(), json!({}))
            .await
            .err(),
        fixture.task(&org_b, &node_a).await.err(),
        fixture.service.get_task(&org_b, &task_a.id).await.err(),
        fixture
            .service
            .cancel_task(&org_b, &member(), &task_a.id)
            .await
            .err(),
    ];

    for refusal in refusals {
        assert!(matches!(refusal, Some(Error::NotFound(_))), "{refusal:?}");
    }

    let task = fixture.service.get_task(&org_a, &task_a.id).await.unwrap();
    assert_eq!(task.state, TaskState::Queued);
    assert!(
        fixture
            .service
            .get_node(&org_a, node_a.id())
            .await
            .unwrap()
            .node
            .revoked_at
            .is_none()
    );
}

#[tokio::test]
async fn a_node_session_cannot_take_or_finish_another_organizations_task() {
    let fixture = Fixture::new();
    let (org_a, org_b) = (org("org-a"), org("org-b"));
    let node_a = fixture.enroll(&org_a, "host-a").await;
    let node_b = fixture.enroll(&org_b, "host-b").await;
    let task_a = fixture.task(&org_a, &node_a).await.unwrap();
    let key = task_a.lease_key();

    let (mut session_b, out) = fixture.open(&node_b).await;
    assert!(
        offers(&out).is_empty(),
        "org-b's node is offered nothing of org-a"
    );

    // Accepting, renewing or finishing the other organization's lease changes nothing.
    let out = fixture
        .service
        .lease_accept(&session_b, key.clone())
        .await
        .unwrap();
    assert_eq!(cancels(&out), std::slice::from_ref(&key));

    let out = fixture
        .service
        .task_result(&session_b, result(&key, TaskStatus::Succeeded))
        .await
        .unwrap();

    assert_eq!(acks(&out), [key.result_id()]);
    assert!(offers(&fixture.service.poll(&mut session_b).await.unwrap()).is_empty());

    let task = fixture.service.get_task(&org_a, &task_a.id).await.unwrap();
    assert_eq!(task.state, TaskState::Queued);
    assert!(task.result.is_none());

    // The task's own node still gets it.
    let (_, out) = fixture.open(&node_a).await;
    assert_eq!(offers(&out), [key]);
}

#[tokio::test]
async fn a_node_does_not_get_a_lease_of_another_node_of_its_organization() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node_1 = fixture.enroll(&org_a, "host-1").await;
    let node_2 = fixture.enroll(&org_a, "host-2").await;
    let task = fixture.task(&org_a, &node_1).await.unwrap();

    let (session_2, out) = fixture
        .connect(
            &node_2,
            super::hello(
                &node_2,
                vec![task.lease_key()],
                vec![task.lease_key().result_id()],
            ),
        )
        .await;

    let session_2 = session_2.unwrap();
    assert_eq!(cancels(&out), [task.lease_key()]);
    assert!(acks(&out).is_empty());

    let wrong_attempt = LeaseKey {
        attempt: 2,
        ..task.lease_key()
    };

    let out = fixture
        .service
        .lease_accept(&session_2, wrong_attempt.clone())
        .await
        .unwrap();
    assert_eq!(cancels(&out), [wrong_attempt]);
}
