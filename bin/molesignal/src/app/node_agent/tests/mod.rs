// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The control plane against in-memory repositories: enrollment, isolation, revocation, leases,
//! results, quota, configuration and drain.

mod enrollment;
mod isolation;
mod leases;
mod lifecycle;

use std::sync::Arc;

use domain::node_agent::{Capabilities, LeaseKey, NodeTask, TaskKind, TaskStatus};
use kernel::ids::Id;
use rcgen::{CertificateParams, KeyPair};
use serde_json::json;

use super::{
    Actor, CreateEnrollmentTokenInput, CreateTaskInput, EnrollInput, EnrollOutcome, HelloInput,
    NodeAgentAuthority, NodeAgentService, Outbound, Session, TaskResultInput, Timings,
    memory::{MemoryAudit, MemoryRepository},
};

pub(super) const SECOND: i64 = 1_000_000;

pub(super) struct Fixture {
    pub service: NodeAgentService,
    pub audit: Arc<MemoryAudit>,
}

/// An enrolled node and the certificate it presents.
pub(super) struct Node {
    pub outcome: EnrollOutcome,
    pub certificate_der: Vec<u8>,
}

impl Node {
    pub fn id(&self) -> &Id {
        &self.outcome.node.id
    }
}

pub(super) fn org(name: &str) -> Id {
    Id::from_string(name)
}

pub(super) fn member() -> Actor {
    Actor {
        kind: "user".into(),
        id: "user-1".into(),
    }
}

pub(super) fn capabilities() -> Capabilities {
    Capabilities {
        task_kinds: vec![TaskKind::AutotracingRun, TaskKind::Profile, TaskKind::Tool],
        features: Vec::new(),
        max_running_tasks: 4,
    }
}

pub(super) fn csr(key: &KeyPair) -> String {
    CertificateParams::new(Vec::<String>::new())
        .unwrap()
        .serialize_request(key)
        .unwrap()
        .pem()
        .unwrap()
}

pub(super) fn der(certificate_pem: &str) -> Vec<u8> {
    pem::parse(certificate_pem).unwrap().contents().to_vec()
}

impl Fixture {
    pub fn new() -> Self {
        let repository = Arc::new(MemoryRepository::default());
        let audit = Arc::new(MemoryAudit::default());
        let authority = NodeAgentAuthority::ephemeral(&["localhost".into()], 30).unwrap();

        let service = NodeAgentService::new(
            repository,
            Arc::new(authority),
            audit.clone(),
            Timings {
                heartbeat_interval_secs: 30,
                lease_ttl_micros: 120 * SECOND,
                offer_ttl_micros: 60 * SECOND,
            },
            "https://localhost:5086".into(),
        );

        Self { service, audit }
    }

    pub async fn token(&self, org_id: &Id, max_uses: u32) -> String {
        self.service
            .create_enrollment_token(
                org_id,
                &member(),
                CreateEnrollmentTokenInput {
                    description: "test".into(),
                    labels: [("rack".to_string(), "r1".to_string())].into(),
                    max_uses,
                    ttl_minutes: 10,
                },
            )
            .await
            .unwrap()
            .issued
            .plaintext
    }

    pub async fn enroll_with(&self, token: &str, host_id: &str) -> kernel::Result<Node> {
        let key = KeyPair::generate().unwrap();

        let outcome = self
            .service
            .enroll(EnrollInput {
                enrollment_token: token.into(),
                csr_pem: csr(&key),
                host_id: host_id.into(),
                boot_id: "boot-1".into(),
                hostname: format!("{host_id}.example"),
                agent_version: "0.4.0".into(),
                protocol_version: "1.0".into(),
                capabilities: capabilities(),
            })
            .await?;

        Ok(Node {
            certificate_der: der(&outcome.certificate_pem),
            outcome,
        })
    }

    pub async fn enroll(&self, org_id: &Id, host_id: &str) -> Node {
        let token = self.token(org_id, 1).await;
        self.enroll_with(&token, host_id).await.unwrap()
    }

    pub async fn connect(
        &self,
        node: &Node,
        hello: HelloInput,
    ) -> (Option<Session>, Vec<Outbound>) {
        let auth = self
            .service
            .authenticate(&node.certificate_der)
            .await
            .unwrap();
        self.service.open_session(auth, hello).await.unwrap()
    }

    pub async fn open_or_none(&self, node: &Node) -> (Option<Session>, Vec<Outbound>) {
        self.connect(node, hello(node, Vec::new(), Vec::new()))
            .await
    }

    pub async fn open(&self, node: &Node) -> (Session, Vec<Outbound>) {
        let (session, out) = self
            .connect(node, hello(node, Vec::new(), Vec::new()))
            .await;
        (session.expect("the session opens"), out)
    }

    pub async fn task(&self, org_id: &Id, node: &Node) -> kernel::Result<NodeTask> {
        self.service
            .create_task(
                org_id,
                &member(),
                CreateTaskInput {
                    node_id: node.id().clone(),
                    kind: TaskKind::Tool,
                    spec: json!({"tool": "dropwatch", "duration_secs": 30}),
                    timeout_seconds: 600,
                    max_attempts: 3,
                },
            )
            .await
    }
}

pub(super) fn hello(
    node: &Node,
    active_leases: Vec<LeaseKey>,
    pending_results: Vec<String>,
) -> HelloInput {
    HelloInput {
        node_id: node.id().0.clone(),
        protocol_version: "1.0".into(),
        agent_version: "0.4.0".into(),
        capabilities: capabilities(),
        applied_config_revision: 0,
        active_leases,
        pending_results,
    }
}

pub(super) fn offers(out: &[Outbound]) -> Vec<LeaseKey> {
    out.iter()
        .filter_map(|message| match message {
            Outbound::LeaseOffer { task, .. } => Some(task.lease_key()),
            _ => None,
        })
        .collect()
}

pub(super) fn cancels(out: &[Outbound]) -> Vec<LeaseKey> {
    out.iter()
        .filter_map(|message| match message {
            Outbound::LeaseCancel { key, .. } => Some(key.clone()),
            _ => None,
        })
        .collect()
}

pub(super) fn acks(out: &[Outbound]) -> Vec<String> {
    out.iter()
        .filter_map(|message| match message {
            Outbound::ResultAck { result_id } => Some(result_id.clone()),
            _ => None,
        })
        .collect()
}

pub(super) fn result(key: &LeaseKey, status: TaskStatus) -> TaskResultInput {
    TaskResultInput {
        result_id: key.result_id(),
        task_id: key.task_id.0.clone(),
        attempt: key.attempt,
        status,
        summary: "events=12".into(),
        artifact_refs: vec!["pprof:sha256:00".into()],
    }
}

/// Accepts the first task offered in `pending` (what the session already got) or by the next poll.
pub(super) async fn run(
    fixture: &Fixture,
    session: &mut Session,
    pending: &[Outbound],
) -> LeaseKey {
    let mut keys = offers(pending);
    keys.extend(offers(&fixture.service.poll(session).await.unwrap()));
    let key = keys.into_iter().next().expect("a task is offered");
    let out = fixture
        .service
        .lease_accept(session, key.clone())
        .await
        .unwrap();
    assert!(out.is_empty(), "{out:?}");
    key
}
