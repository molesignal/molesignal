// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `NodeControl` over a real TLS listener: enrollment without a client certificate, a session with
//! the issued one, an offer accepted and finished, a repeated result, and a revocation.

use std::{sync::Arc, time::Duration};

use domain::node_agent::{TaskKind, TaskState};
use kernel::ids::Id;
use protocol::node_control::v1::{
    self as wire, agent_message, control_message, node_control_client::NodeControlClient,
};
use rcgen::{CertificateParams, KeyPair};
use serde_json::json;
use tokio::{net::TcpListener, sync::mpsc};
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::{
    Code, Streaming,
    transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity, Server},
};

use super::{NodeControlGrpc, server_tls_config};
use crate::app::node_agent::{
    Actor, CreateEnrollmentTokenInput, CreateTaskInput, NodeAgentAuthority, NodeAgentService,
    Timings,
    memory::{MemoryAudit, MemoryRepository},
};

fn member() -> Actor {
    Actor {
        kind: "user".into(),
        id: "user-1".into(),
    }
}

async fn start() -> (Arc<NodeAgentService>, String, String) {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let authority = NodeAgentAuthority::ephemeral(&["localhost".into()], 30).unwrap();
    let ca = authority.ca_certificate_pem().to_string();
    let tls = server_tls_config(&authority);

    let service = Arc::new(NodeAgentService::new(
        Arc::new(MemoryRepository::default()),
        Arc::new(authority),
        Arc::new(MemoryAudit::default()),
        Timings {
            heartbeat_interval_secs: 30,
            lease_ttl_micros: 120_000_000,
            offer_ttl_micros: 60_000_000,
        },
        "https://localhost".into(),
    ));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!(
        "https://localhost:{}",
        listener.local_addr().unwrap().port()
    );
    let server = NodeControlGrpc::new(service.clone()).into_server();

    tokio::spawn(
        Server::builder()
            .tls_config(tls)
            .unwrap()
            .add_service(server)
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );

    (service, address, ca)
}

async fn client(
    address: &str,
    ca: &str,
    identity: Option<(&str, &str)>,
) -> NodeControlClient<Channel> {
    let mut tls = ClientTlsConfig::new()
        .ca_certificate(Certificate::from_pem(ca))
        .domain_name("localhost");

    if let Some((certificate, key)) = identity {
        tls = tls.identity(Identity::from_pem(certificate, key));
    }

    let channel = Endpoint::from_shared(address.to_string())
        .unwrap()
        .tls_config(tls)
        .unwrap()
        .connect()
        .await
        .unwrap();

    NodeControlClient::new(channel)
}

fn send(sender: &mpsc::Sender<wire::AgentMessage>, payload: agent_message::Payload) {
    sender
        .try_send(wire::AgentMessage {
            payload: Some(payload),
        })
        .unwrap();
}

async fn next(stream: &mut Streaming<wire::ControlMessage>) -> control_message::Payload {
    tokio::time::timeout(Duration::from_secs(10), stream.message())
        .await
        .expect("a control message in time")
        .expect("no stream error")
        .expect("the stream is open")
        .payload
        .expect("a payload")
}

#[tokio::test]
async fn enroll_session_lease_result_and_revoke_over_mtls() {
    let (service, address, ca) = start().await;
    let org_a = Id::from_string("org-a");

    let token = service
        .create_enrollment_token(
            &org_a,
            &member(),
            CreateEnrollmentTokenInput {
                description: String::new(),
                labels: Default::default(),
                max_uses: 1,
                ttl_minutes: 5,
            },
        )
        .await
        .unwrap()
        .issued
        .plaintext;

    // Enrollment needs no client certificate.
    let key = KeyPair::generate().unwrap();
    let csr = CertificateParams::new(Vec::<String>::new())
        .unwrap()
        .serialize_request(&key)
        .unwrap();
    let capabilities = wire::Capabilities {
        task_kinds: vec!["tool".into(), "future_kind".into()],
        features: Vec::new(),
        max_running_tasks: 2,
    };

    let enrolled = client(&address, &ca, None)
        .await
        .enroll(wire::EnrollRequest {
            enrollment_token: token,
            csr_pem: csr.pem().unwrap(),
            host_id: "host-1".into(),
            boot_id: "boot-1".into(),
            agent_version: "0.4.0".into(),
            protocol_version: "1.0".into(),
            capabilities: Some(capabilities.clone()),
            hostname: "host-1.example".into(),
        })
        .await
        .unwrap()
        .into_inner();

    assert_eq!(enrolled.protocol_version, "1.0");
    let node_id = Id::from_string(enrolled.node_id.clone());

    // Without a client certificate there is no session.
    let (_sender, receiver) = mpsc::channel(8);
    let refused = client(&address, &ca, None)
        .await
        .session(ReceiverStream::new(receiver))
        .await
        .unwrap_err();

    assert_eq!(refused.code(), Code::Unauthenticated);

    let task = service
        .create_task(
            &org_a,
            &member(),
            CreateTaskInput {
                node_id: node_id.clone(),
                kind: TaskKind::Tool,
                spec: json!({"tool": "tcpshark", "mode": "retransmit", "duration_secs": 5}),
                timeout_seconds: 300,
                max_attempts: 2,
            },
        )
        .await
        .unwrap();

    let private_key = key.serialize_pem();
    let mut node = client(
        &address,
        &ca,
        Some((&enrolled.certificate_pem, &private_key)),
    )
    .await;
    let (sender, receiver) = mpsc::channel(16);

    send(
        &sender,
        agent_message::Payload::Hello(wire::Hello {
            node_id: enrolled.node_id.clone(),
            protocol_version: "1.0".into(),
            agent_version: "0.4.0".into(),
            capabilities: Some(capabilities),
            applied_config_revision: 0,
            active_leases: Vec::new(),
            pending_results: Vec::new(),
        }),
    );

    let mut stream = node
        .session(ReceiverStream::new(receiver))
        .await
        .unwrap()
        .into_inner();

    let control_message::Payload::Welcome(welcome) = next(&mut stream).await else {
        panic!("Welcome first");
    };

    assert_eq!(
        (
            welcome.protocol_version.as_str(),
            welcome.heartbeat_interval_seconds
        ),
        ("1.0", 30)
    );

    let control_message::Payload::LeaseOffer(offer) = next(&mut stream).await else {
        panic!("the queued task is offered");
    };

    assert_eq!(
        (offer.task_id.as_str(), offer.attempt, offer.kind.as_str()),
        (task.id.as_str(), 1, "tool")
    );
    assert!(offer.lease_expires_at > offer.server_time && offer.lease_expires_at <= offer.deadline);
    let spec: serde_json::Value = serde_json::from_slice(&offer.spec).unwrap();
    assert_eq!(spec["tool"], "tcpshark");

    send(
        &sender,
        agent_message::Payload::LeaseAccept(wire::LeaseAccept {
            task_id: offer.task_id.clone(),
            attempt: 1,
        }),
    );

    let result = wire::TaskResult {
        result_id: format!("{}/1", offer.task_id),
        task_id: offer.task_id.clone(),
        attempt: 1,
        status: wire::TaskStatus::Succeeded as i32,
        summary: "events=4".into(),
        artifact_refs: Vec::new(),
    };

    // The same result twice: both acknowledged, recorded once.
    for _ in 0..2 {
        send(&sender, agent_message::Payload::TaskResult(result.clone()));

        let control_message::Payload::ResultAck(ack) = next(&mut stream).await else {
            panic!("the result is acknowledged");
        };

        assert_eq!(ack.result_id, result.result_id);
    }

    let finished = service.get_task(&org_a, &task.id).await.unwrap();
    assert_eq!(finished.state, TaskState::Succeeded);
    let received_at = finished.result.unwrap().received_at;

    service
        .revoke(&org_a, &member(), &node_id, "decommissioned")
        .await
        .unwrap();

    let control_message::Payload::Revoke(revoke) = next(&mut stream).await else {
        panic!("the session is told of the revocation");
    };

    assert_eq!(revoke.reason, "decommissioned");
    let end = tokio::time::timeout(Duration::from_secs(5), stream.message())
        .await
        .unwrap();
    assert!(matches!(end, Ok(None)), "{end:?}");

    let again = service.get_task(&org_a, &task.id).await.unwrap();
    assert_eq!(again.result.unwrap().received_at, received_at);

    // A revoked certificate cannot rotate.
    let new_key = KeyPair::generate().unwrap();
    let new_csr = CertificateParams::new(Vec::<String>::new())
        .unwrap()
        .serialize_request(&new_key)
        .unwrap();

    let refused = node
        .rotate_certificate(wire::RotateCertificateRequest {
            csr_pem: new_csr.pem().unwrap(),
        })
        .await
        .unwrap_err();

    assert_eq!(refused.code(), Code::Unauthenticated);
}
