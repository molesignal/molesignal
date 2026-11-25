// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use kernel::Error;
use rcgen::KeyPair;

use super::{Fixture, csr, der, hello, org};
use crate::app::node_agent::{Outbound, peer_identity};

#[tokio::test]
async fn enrollment_issues_an_identity_bound_to_the_organization() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;

    let identity = peer_identity(&node.certificate_der).unwrap();
    assert_eq!(identity.node_id, node.id().0);
    assert_eq!(identity.org_id, "org-a");
    assert_eq!(identity.serial, node.outcome.node.certificate_serial);
    assert!(node.outcome.ca_bundle_pem.contains("BEGIN CERTIFICATE"));
    assert_eq!(
        node.outcome.node.labels.get("rack").map(String::as_str),
        Some("r1")
    );

    let auth = fixture
        .service
        .authenticate(&node.certificate_der)
        .await
        .unwrap();
    assert_eq!(&auth.node.id, node.id());
    assert!(auth.used_current);

    assert_eq!(
        fixture.audit.actions(&org_a),
        ["node_agent.enrollment_token.create", "node_agent.enroll"]
    );
}

#[tokio::test]
async fn a_token_enrolls_only_as_many_nodes_as_it_allows() {
    let fixture = Fixture::new();
    let token = fixture.token(&org("org-a"), 1).await;
    fixture.enroll_with(&token, "host-1").await.unwrap();

    let error = fixture.enroll_with(&token, "host-2").await.err().unwrap();
    assert!(matches!(error, Error::Unauthorized(_)), "{error:?}");

    let error = fixture
        .enroll_with(&"x".repeat(40), "host-3")
        .await
        .err()
        .unwrap();
    assert!(matches!(error, Error::Unauthorized(_)), "{error:?}");
}

#[tokio::test]
async fn a_revoked_token_enrolls_nothing() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let token = fixture.token(&org_a, 5).await;
    let token_id = fixture
        .service
        .list_enrollment_tokens(&org_a)
        .await
        .unwrap()[0]
        .id
        .clone();

    fixture
        .service
        .revoke_enrollment_token(&org_a, &super::member(), &token_id)
        .await
        .unwrap();

    assert!(fixture.enroll_with(&token, "host-1").await.is_err());
    assert!(
        fixture
            .audit
            .actions(&org_a)
            .contains(&"node_agent.enrollment_token.revoke".to_string())
    );
}

#[tokio::test]
async fn enrolling_the_same_host_again_keeps_the_node_and_replaces_the_certificate() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let first = fixture.enroll(&org_a, "host-1").await;
    let second = fixture.enroll(&org_a, "host-1").await;

    assert_eq!(first.id(), second.id());
    assert_ne!(
        first.outcome.node.certificate_serial,
        second.outcome.node.certificate_serial
    );
    assert!(
        fixture
            .service
            .authenticate(&first.certificate_der)
            .await
            .is_err()
    );
    assert!(
        fixture
            .service
            .authenticate(&second.certificate_der)
            .await
            .is_ok()
    );
    assert!(
        fixture
            .audit
            .actions(&org_a)
            .contains(&"node_agent.reenroll".to_string())
    );
}

#[tokio::test]
async fn another_major_version_is_refused_at_enrollment_and_in_the_session() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let token = fixture.token(&org_a, 2).await;
    let key = KeyPair::generate().unwrap();

    let error = fixture
        .service
        .enroll(crate::app::node_agent::EnrollInput {
            enrollment_token: token.clone(),
            csr_pem: csr(&key),
            host_id: "host-1".into(),
            boot_id: String::new(),
            hostname: String::new(),
            agent_version: "9.0.0".into(),
            protocol_version: "2.0".into(),
            capabilities: super::capabilities(),
        })
        .await
        .unwrap_err();

    assert!(
        error.to_string().contains("UNSUPPORTED_PROTOCOL"),
        "{error}"
    );

    let node = fixture.enroll_with(&token, "host-1").await.unwrap();
    let mut hello = hello(&node, Vec::new(), Vec::new());
    hello.protocol_version = "2.1".into();
    let (session, out) = fixture.connect(&node, hello).await;

    assert!(session.is_none());
    assert!(
        matches!(&out[0], Outbound::Welcome { protocol_version, .. } if protocol_version == "1.0")
    );
    assert!(
        matches!(&out[1], Outbound::Close(error) if error.to_string().contains("UNSUPPORTED_PROTOCOL"))
    );
}

#[tokio::test]
async fn a_hello_for_another_node_is_refused() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;
    let other = fixture.enroll(&org_a, "host-2").await;

    let auth = fixture
        .service
        .authenticate(&node.certificate_der)
        .await
        .unwrap();
    let error = fixture
        .service
        .open_session(auth, hello(&other, Vec::new(), Vec::new()))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Unauthorized(_)), "{error:?}");
}

#[tokio::test]
async fn a_rotated_certificate_replaces_the_old_one_once_it_is_used() {
    let fixture = Fixture::new();
    let org_a = org("org-a");
    let node = fixture.enroll(&org_a, "host-1").await;
    let key = KeyPair::generate().unwrap();

    let rotated = fixture
        .service
        .rotate_certificate(&node.certificate_der, &csr(&key))
        .await
        .unwrap();

    let new_der = der(&rotated.certificate_pem);

    // Until a session uses the new certificate, the old one still works.
    let old = fixture
        .service
        .authenticate(&node.certificate_der)
        .await
        .unwrap();
    assert!(!old.used_current);

    let new = fixture.service.authenticate(&new_der).await.unwrap();
    assert!(new.used_current);
    fixture
        .service
        .open_session(new, hello(&node, Vec::new(), Vec::new()))
        .await
        .unwrap();

    assert!(
        fixture
            .service
            .authenticate(&node.certificate_der)
            .await
            .is_err()
    );
    assert!(fixture.service.authenticate(&new_der).await.is_ok());
    assert!(
        fixture
            .audit
            .actions(&org_a)
            .contains(&"node_agent.certificate.rotate".to_string())
    );
}

#[tokio::test]
async fn a_csr_that_does_not_parse_is_refused() {
    let fixture = Fixture::new();
    let token = fixture.token(&org("org-a"), 1).await;
    let key = KeyPair::generate().unwrap();
    // Change one byte of the signed part: the signature no longer matches.
    let mut csr_der = pem::parse(csr(&key)).unwrap().contents().to_vec();
    let middle = csr_der.len() / 2;
    csr_der[middle] ^= 0x01;
    let broken = pem::encode(&pem::Pem::new("CERTIFICATE REQUEST", csr_der));

    let error = fixture
        .service
        .enroll(crate::app::node_agent::EnrollInput {
            enrollment_token: token,
            csr_pem: broken,
            host_id: "host-1".into(),
            boot_id: String::new(),
            hostname: String::new(),
            agent_version: "0.4.0".into(),
            protocol_version: "1.0".into(),
            capabilities: super::capabilities(),
        })
        .await
        .unwrap_err();

    assert!(matches!(error, Error::InvalidArgument(_)), "{error:?}");
}
