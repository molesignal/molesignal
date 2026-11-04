// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The Agent's side of the control stream, over real mutual TLS and without a database.
//!
//! The server reads Hello before it answers the call (`control_stream` in
//! `bin/molesignal/src/api/grpc/probe/mod.rs`), so an Agent that waits for the answer first stays
//! `registered` for good. The service here keeps that order and nothing else of the real one;
//! `bin/molesignal/tests/bootstrap_it_probe_agent.rs` runs the Agent against the real one.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, SubjectPublicKeyInfo,
};
use tokio::{net::TcpListener, sync::mpsc};
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::{
    Request, Response, Status, Streaming,
    transport::{Certificate, Identity, Server, ServerTlsConfig},
};

use super::{RunOptions, run};
use crate::{
    identity::{AgentIdentity, PendingKey, StoredIdentity},
    protocol::v1::{
        AgentFrame, ControlFrame, HelloAck, RegisterRequest, RegisterResponse, agent_frame,
        control_frame,
        probe_service_server::{ProbeService, ProbeServiceServer},
    },
};

const AGENT_ID: &str = "agent-under-test";

/// An Agent that waits for the answer never sends a frame, so this only has to outlast a slow
/// machine: it decides how long a failure takes, never whether a working Agent passes.
const FRAME_TIMEOUT: Duration = Duration::from_secs(30);

struct ReadsHelloFirst {
    frames: mpsc::UnboundedSender<AgentFrame>,
}

#[tonic::async_trait]
impl ProbeService for ReadsHelloFirst {
    async fn register(
        &self,
        _request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        Err(Status::unimplemented(
            "this test only opens control streams",
        ))
    }

    type ControlStreamStream = ReceiverStream<Result<ControlFrame, Status>>;

    async fn control_stream(
        &self,
        request: Request<Streaming<AgentFrame>>,
    ) -> Result<Response<Self::ControlStreamStream>, Status> {
        if request.peer_certs().is_none() {
            return Err(Status::unauthenticated("no client certificate"));
        }

        let mut input = request.into_inner();

        // Nothing goes back to the Agent, not even the response headers, until Hello has arrived.
        let hello = input
            .message()
            .await?
            .ok_or_else(|| Status::invalid_argument("stream ended before Hello"))?;

        let frames = self.frames.clone();
        let _ = frames.send(hello);
        let (output, receiver) = mpsc::channel(1);

        output
            .send(Ok(ControlFrame {
                frame_id: "hello-ack".into(),
                payload: Some(control_frame::Payload::HelloAck(HelloAck {
                    protocol_version: 1,
                    server_time_micros: 0,
                    heartbeat_interval_secs: 15,
                    lease_renewal_interval_secs: 30,
                    max_in_flight: 1,
                })),
            }))
            .await
            .map_err(|_| Status::cancelled("the Agent went away"))?;

        // The answer stays open for as long as the Agent keeps sending.
        tokio::spawn(async move {
            while let Ok(Some(frame)) = input.message().await {
                let _ = frames.send(frame);
            }

            drop(output);
        });

        Ok(Response::new(ReceiverStream::new(receiver)))
    }
}

struct Pki {
    ca_pem: String,
    server_certificate_pem: String,
    server_key_pem: String,
    agent_certificate_pem: String,
    agent_key_pem: String,
}

/// Issued like `ProbeCertificateAuthority`: the Agent makes its key, the CA signs the public half.
fn pki() -> Pki {
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).expect("CA parameters");

    ca_params
        .distinguished_name
        .push(DnType::CommonName, "Test Probe CA");

    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

    ca_params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyCertSign,
    ];

    let ca_key = KeyPair::generate().expect("CA key");
    let ca_pem = ca_params.self_signed(&ca_key).expect("self-sign CA").pem();
    let issuer = Issuer::new(ca_params, ca_key);

    let mut server_params =
        CertificateParams::new(vec!["localhost".to_string()]).expect("server parameters");
    server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];

    let server_key = KeyPair::generate().expect("server key");

    let server_certificate = server_params
        .signed_by(&server_key, &issuer)
        .expect("sign server certificate");

    let agent_key = PendingKey::generate().expect("Agent key");
    let public_key = SubjectPublicKeyInfo::from_der(&agent_key.public_key_der()).expect("SPKI");
    let mut agent_params = CertificateParams::new(Vec::<String>::new()).expect("Agent parameters");

    agent_params
        .distinguished_name
        .push(DnType::CommonName, AGENT_ID);

    agent_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    agent_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];

    let agent_certificate = agent_params
        .signed_by(&public_key, &issuer)
        .expect("sign Agent certificate");

    Pki {
        server_certificate_pem: format!("{}{ca_pem}", server_certificate.pem()),
        server_key_pem: server_key.serialize_pem(),
        agent_certificate_pem: format!("{}{ca_pem}", agent_certificate.pem()),
        agent_key_pem: agent_key.private_key_pem(),
        ca_pem,
    }
}

struct StateDir(PathBuf);

impl StateDir {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "probe-agent-control-stream-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        )))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for StateDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn next_frame(
    frames: &mut mpsc::UnboundedReceiver<AgentFrame>,
    waiting_for: &str,
) -> AgentFrame {
    tokio::time::timeout(FRAME_TIMEOUT, frames.recv())
        .await
        .unwrap_or_else(|_| panic!("no {waiting_for} from the Agent in {FRAME_TIMEOUT:?}"))
        .expect("the server is still running")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn agent_sends_hello_without_waiting_for_the_answer() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

    let pki = pki();
    let state = StateDir::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("local address").port();

    AgentIdentity {
        metadata: StoredIdentity {
            agent_id: AGENT_ID.into(),
            location_id: "location-under-test".into(),
            control_endpoint: format!("https://localhost:{port}"),
            // Far enough away that the Agent asks for no rotation between Hello and the heartbeat.
            certificate_expires_at_micros: i64::MAX,
            protocol_version: 1,
        },
        private_key_pem: pki.agent_key_pem,
        certificate_chain_pem: pki.agent_certificate_pem,
        ca_certificate_pem: pki.ca_pem.clone(),
    }
    .persist(state.path())
    .expect("write the Agent identity");

    let (frames, mut received) = mpsc::unbounded_channel();

    let tls = ServerTlsConfig::new()
        .identity(Identity::from_pem(
            pki.server_certificate_pem,
            pki.server_key_pem,
        ))
        .client_ca_root(Certificate::from_pem(pki.ca_pem));

    let server = tokio::spawn(
        Server::builder()
            .tls_config(tls)
            .expect("server TLS")
            .add_service(ProbeServiceServer::new(ReadsHelloFirst { frames }))
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );

    let agent = tokio::spawn(run(RunOptions {
        state_dir: state.path().to_path_buf(),
        max_concurrent: 1,
        max_browser_concurrent: 0,
    }));

    let first = next_frame(&mut received, "Hello").await;

    let Some(agent_frame::Payload::Hello(hello)) = first.payload else {
        panic!("the first frame must be Hello, got {:?}", first.payload);
    };

    assert_eq!(hello.agent_id, AGENT_ID);
    assert_eq!(hello.protocol_version, 1);

    // The Agent checks that the first thing back is a HelloAck, and only then starts its heartbeat.
    let second = next_frame(&mut received, "heartbeat after HelloAck").await;
    assert!(
        matches!(second.payload, Some(agent_frame::Payload::Heartbeat(_))),
        "expected a heartbeat after HelloAck, got {:?}",
        second.payload
    );

    agent.abort();
    server.abort();
}
