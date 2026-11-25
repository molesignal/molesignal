// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Revocation, drain and configuration.

use aws_lc_rs::signature::{ECDSA_P256_SHA256_ASN1, UnparsedPublicKey};
use domain::node_agent::{ConfigState, NodeStatus, TaskState};
use kernel::Error;
use rcgen::KeyPair;
use serde_json::json;
use x509_cert::der::Decode as _;

use super::{Fixture, csr, der, member, offers, org, run};
use crate::app::node_agent::Outbound;

#[tokio::test]
async fn revoking_a_node_ends_its_tasks_tells_its_session_and_refuses_its_certificate() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;
    let running = fixture.task(&org_a, &node).await.unwrap();
    let queued = fixture.task(&org_a, &node).await.unwrap();
    let (mut session, _) = fixture.open(&node).await;
    fixture
        .service
        .lease_accept(&session, running.lease_key())
        .await
        .unwrap();

    let view = fixture
        .service
        .revoke(&org_a, &member(), node.id(), "key leaked")
        .await
        .unwrap();
    assert_eq!(view.status, NodeStatus::Revoked);

    for task in [&running, &queued] {
        let task = fixture.service.get_task(&org_a, &task.id).await.unwrap();
        assert_eq!(task.state, TaskState::Cancelled);
        assert_eq!(task.failure.as_deref(), Some("the node was revoked"));
    }

    // The live session is told and ends.
    let out = fixture.service.poll(&mut session).await.unwrap();
    assert!(
        matches!(&out[..], [Outbound::Revoke { reason }, Outbound::End] if reason == "key leaked")
    );

    // A new session is told the same; nothing else works with the certificate.
    let (session, out) = fixture.open_or_none(&node).await;
    assert!(session.is_none());
    assert!(matches!(&out[0], Outbound::Revoke { .. }));

    let key = KeyPair::generate().unwrap();
    let error = fixture
        .service
        .rotate_certificate(&node.certificate_der, &csr(&key))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Unauthorized(_)), "{error:?}");

    let error = fixture.task(&org_a, &node).await.unwrap_err();
    assert!(matches!(error, Error::Conflict(_)), "{error:?}");

    let actions = fixture.audit.actions(&org_a);
    assert!(
        actions.contains(&"node_agent.revoke".to_string()),
        "{actions:?}"
    );

    // Enrolling the host again is the way back.
    let again = fixture.enroll(&org_a, "host-1").await;
    assert_eq!(again.id(), node.id());
    let view = fixture.service.get_node(&org_a, node.id()).await.unwrap();
    assert!(view.node.revoked_at.is_none());
    assert!(
        fixture
            .service
            .authenticate(&node.certificate_der)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn a_draining_node_gets_no_new_tasks_and_a_lifted_drain_restarts_its_session() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;
    fixture.task(&org_a, &node).await.unwrap();
    let (mut session, out) = fixture.open(&node).await;
    let running = run(&fixture, &mut session, &out).await;

    fixture
        .service
        .drain(&org_a, &member(), node.id(), 300)
        .await
        .unwrap();
    let out = fixture.service.poll(&mut session).await.unwrap();
    assert!(matches!(&out[..], [Outbound::Drain { .. }]), "{out:?}");

    // Running tasks keep their lease; new ones are refused.
    let error = fixture.task(&org_a, &node).await.unwrap_err();
    assert!(matches!(error, Error::Conflict(_)), "{error:?}");

    let out = fixture
        .service
        .lease_accept(&session, running.clone())
        .await
        .unwrap();

    assert!(out.is_empty());

    fixture.service.drained(&session).await.unwrap();
    let view = fixture.service.get_node(&org_a, node.id()).await.unwrap();
    assert_eq!(view.status, NodeStatus::Drained);

    fixture
        .service
        .resume(&org_a, &member(), node.id())
        .await
        .unwrap();
    let out = fixture.service.poll(&mut session).await.unwrap();
    assert!(
        matches!(&out[..], [Outbound::Close(Error::Unavailable(_))]),
        "{out:?}"
    );

    let actions = fixture.audit.actions(&org_a);

    for action in [
        "node_agent.drain",
        "node_agent.drained",
        "node_agent.resume",
    ] {
        assert!(
            actions.contains(&action.to_string()),
            "{action}: {actions:?}"
        );
    }

    let (_, out) = fixture.open(&node).await;
    assert!(
        !out.iter()
            .any(|message| matches!(message, Outbound::Drain { .. }))
    );
    assert!(
        offers(&out).is_empty(),
        "the running task is not offered again"
    );
}

#[tokio::test]
async fn a_configuration_goes_out_once_signed_by_the_ca_and_its_status_is_kept() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;
    let (mut session, _) = fixture.open(&node).await;

    let view = fixture
        .service
        .set_config(
            &org_a,
            &member(),
            node.id(),
            json!({"autotracing": {"cpu_sys": true}}),
        )
        .await
        .unwrap();

    assert_eq!(view.node.desired_config_revision(), 1);

    let out = fixture.service.poll(&mut session).await.unwrap();

    let [
        Outbound::ConfigUpdate {
            revision,
            config,
            signature,
        },
    ] = &out[..]
    else {
        panic!("{out:?}");
    };

    assert_eq!(*revision, 1);
    assert!(fixture.service.poll(&mut session).await.unwrap().is_empty());

    // The agent checks the signature with the CA certificate it got at enrollment.
    let ca = x509_cert::Certificate::from_der(&der(&node.outcome.ca_bundle_pem)).unwrap();
    let public_key = ca
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .raw_bytes();
    let mut message = revision.to_be_bytes().to_vec();
    message.extend_from_slice(config);

    UnparsedPublicKey::new(&ECDSA_P256_SHA256_ASN1, public_key)
        .verify(&message, signature)
        .expect("the configuration is signed by the CA key");

    fixture
        .service
        .config_status(
            &session,
            1,
            ConfigState::RolledBack,
            Some("health degraded".into()),
        )
        .await
        .unwrap();

    let view = fixture.service.get_node(&org_a, node.id()).await.unwrap();
    let status = view.node.config_status.unwrap();
    assert_eq!(
        (status.revision, status.state),
        (1, ConfigState::RolledBack)
    );
    assert!(
        fixture
            .audit
            .actions(&org_a)
            .contains(&"node_agent.config.update".to_string())
    );

    // A node that already applied the revision is not sent it again.
    let mut hello = super::hello(&node, Vec::new(), Vec::new());
    hello.applied_config_revision = 1;
    let (_, out) = fixture.connect(&node, hello).await;
    assert!(
        !out.iter()
            .any(|message| matches!(message, Outbound::ConfigUpdate { .. }))
    );
}

#[tokio::test]
async fn tasks_need_a_kind_the_node_declares_and_a_sound_spec() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let token = fixture.token(&org_a, 1).await;
    let key = KeyPair::generate().unwrap();

    let mut input = crate::app::node_agent::EnrollInput {
        enrollment_token: token,
        csr_pem: csr(&key),
        host_id: "host-1".into(),
        boot_id: String::new(),
        hostname: String::new(),
        agent_version: "0.4.0".into(),
        protocol_version: "1.0".into(),
        capabilities: super::capabilities(),
    };

    input.capabilities.task_kinds = vec![domain::node_agent::TaskKind::Profile];
    let outcome = fixture.service.enroll(input).await.unwrap();

    let error = fixture
        .service
        .create_task(
            &org_a,
            &member(),
            crate::app::node_agent::CreateTaskInput {
                node_id: outcome.node.id.clone(),
                kind: domain::node_agent::TaskKind::Tool,
                spec: json!({"tool": "perf", "duration_secs": 10}),
                timeout_seconds: 600,
                max_attempts: 1,
            },
        )
        .await
        .unwrap_err();

    assert!(matches!(error, Error::InvalidArgument(_)), "{error:?}");

    let task = fixture
        .service
        .create_task(
            &org_a,
            &member(),
            crate::app::node_agent::CreateTaskInput {
                node_id: outcome.node.id.clone(),
                kind: domain::node_agent::TaskKind::Profile,
                spec: json!({"duration_secs": 30, "type": "cpu"}),
                timeout_seconds: 600,
                max_attempts: 1,
            },
        )
        .await
        .unwrap();

    assert_eq!(task.state, TaskState::Queued);
    assert!(task.deadline.0 > task.created_at.0);
    assert!(
        fixture
            .audit
            .actions(&org_a)
            .contains(&"node_agent.task.create".to_string())
    );
}
