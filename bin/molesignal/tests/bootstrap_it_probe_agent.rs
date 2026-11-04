// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! An external Probe Agent against the real gRPC listeners and a real Postgres: registration with
//! both kinds of token, then the control stream over mutual TLS until the Agent is online.
//!
//! The built-in Local Location goes through none of this, so nothing else in the suite covers it.
//! Needs `MS_RUN_IT=1`, plus `MS_IT_POSTGRES_DSN` on a machine without Docker.

mod common;

use std::{
    path::Path,
    time::{Duration, Instant},
};

use base64::Engine as _;
use common::TestServer;
use molesignal::{api::grpc::serve_probe_grpc, config::ProbeGrpcSettings};
use probe_agent::{
    executor::FEATURE_LEVEL,
    identity::AgentIdentity,
    runtime::{RegisterOptions, RunOptions, register, run},
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tonic::{Code, Status};

/// A stuck Agent stays stuck, so this only has to outlast a slow machine: it decides how long a
/// failure takes, never whether a working Agent passes.
const ONLINE_TIMEOUT: Duration = Duration::from_secs(30);

struct Api<'a> {
    server: &'a TestServer,
}

impl Api<'_> {
    async fn post(&self, path: &str, body: Value) -> Value {
        let (name, value) = self.server.auth_header();

        let response = self
            .server
            .client
            .post(format!("{}/api/v1{path}", self.server.base_url))
            .header(name, value)
            .json(&body)
            .send()
            .await
            .expect("send the request");

        read_json(path, response).await
    }

    async fn agent(&self, location_id: &str, agent_id: &str) -> Value {
        let path = format!("/synthetics/locations/{location_id}/agents");
        let (name, value) = self.server.auth_header();

        let response = self
            .server
            .client
            .get(format!("{}/api/v1{path}", self.server.base_url))
            .header(name, value)
            .send()
            .await
            .expect("send the request");

        read_json(&path, response)
            .await
            .as_array()
            .expect("a list of Agents")
            .iter()
            .find(|agent| agent["id"] == agent_id)
            .unwrap_or_else(|| panic!("Agent {agent_id} is not in Location {location_id}"))
            .clone()
    }
}

async fn read_json(path: &str, response: reqwest::Response) -> Value {
    let status = response.status();
    let body = response.text().await.expect("read the response");
    assert!(status.is_success(), "{path}: {status} {body}");

    serde_json::from_str(&body).expect("a JSON response")
}

/// What an operator copies from the page into `probe-agent register`.
struct Instructions {
    token: String,
    endpoint: String,
    ca_certificate_base64: String,
}

impl Instructions {
    fn from_response(body: &Value, token_field: &str) -> Self {
        let ca_certificate_pem = body["caCertificatePem"].as_str().expect("the Probe CA");

        Self {
            token: body[token_field].as_str().expect("the token").to_string(),
            endpoint: body["registerEndpoint"]
                .as_str()
                .expect("the register endpoint")
                .to_string(),
            ca_certificate_base64: base64::engine::general_purpose::STANDARD
                .encode(ca_certificate_pem),
        }
    }

    async fn register(&self, state_dir: &Path) -> anyhow::Result<()> {
        register(RegisterOptions {
            state_dir: state_dir.to_path_buf(),
            endpoint: self.endpoint.clone(),
            token: self.token.clone(),
            ca_certificate_base64: self.ca_certificate_base64.clone(),
            hostname: Some("probe-under-test".into()),
        })
        .await
    }

    /// A broken query answers `Internal` here, whatever the token.
    async fn assert_refused(&self, what: &str) {
        let state = state_dir();

        let error = self
            .register(state.path())
            .await
            .expect_err(&format!("{what} must not register an Agent"));

        let status = error
            .downcast_ref::<Status>()
            .unwrap_or_else(|| panic!("{what}: not an answer from the server: {error:#}"));

        assert_eq!(status.code(), Code::Unauthenticated, "{what}: {error:#}");
    }
}

fn state_dir() -> TempDir {
    tempfile::tempdir().expect("an Agent state directory")
}

/// Held together so the two come out different. A port can still be taken before the server binds
/// it, because `serve_probe_grpc` binds by number.
fn free_ports() -> (u16, u16) {
    let first = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
    let second = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");

    (
        first.local_addr().expect("local address").port(),
        second.local_addr().expect("local address").port(),
    )
}

async fn wait_for_listener(port: u16) {
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            return;
        }

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    panic!("nothing listens on port {port}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn external_agent_registers_and_goes_online() {
    if common::skip_unless_enabled() {
        eprintln!("skipped (set MS_RUN_IT=1 to enable)");
        return;
    }

    let (register_port, control_port) = free_ports();

    let server = TestServer::start_with_probe(ProbeGrpcSettings {
        bind: "127.0.0.1".into(),
        register_port,
        control_port,
        ..Default::default()
    })
    .await;

    let state = server.state.clone();
    let settings = server.settings.probe.clone();
    let listeners = tokio::spawn(async move { serve_probe_grpc(state, &settings).await });

    wait_for_listener(register_port).await;
    wait_for_listener(control_port).await;

    let api = Api { server: &server };

    let location = api
        .post(
            "/synthetics/locations",
            json!({ "name": "Test pool", "code": "test-pool" }),
        )
        .await;

    let location_id = location["id"].as_str().expect("the Location id");

    let registered = a_one_time_token_registers_once(&api, location_id, control_port).await;
    an_agent_token_registers_until_disabled(&api, location_id).await;
    a_registered_agent_goes_online(&api, location_id, registered.path()).await;

    listeners.abort();
}

async fn a_one_time_token_registers_once(
    api: &Api<'_>,
    location_id: &str,
    control_port: u16,
) -> TempDir {
    let body = api
        .post(
            &format!("/synthetics/locations/{location_id}/register-tokens"),
            json!({ "ttlMinutes": 15 }),
        )
        .await;

    let instructions = Instructions::from_response(&body, "registerToken");
    let state = state_dir();

    instructions
        .register(state.path())
        .await
        .expect("a fresh one-time token registers an Agent");

    let identity = AgentIdentity::load(state.path()).expect("the Agent identity was written");
    assert_eq!(identity.metadata.location_id, location_id);
    assert_eq!(
        identity.metadata.control_endpoint,
        format!("https://localhost:{control_port}")
    );

    let agent = api.agent(location_id, &identity.metadata.agent_id).await;
    assert_eq!(agent["status"], "registered");

    instructions.assert_refused("a used one-time token").await;

    Instructions {
        token: "a-token-nobody-issued-of-a-plausible-length".into(),
        ..instructions
    }
    .assert_refused("an unknown token")
    .await;

    state
}

async fn an_agent_token_registers_until_disabled(api: &Api<'_>, location_id: &str) {
    let body = api
        .post(
            "/synthetics/agent-tokens",
            json!({ "name": "Test token", "locationId": location_id }),
        )
        .await;

    let instructions = Instructions::from_response(&body, "agentToken");

    // Unlike a one-time token, one Agent Token serves every Agent of the pool.
    for _ in 0..2 {
        let state = state_dir();

        instructions
            .register(state.path())
            .await
            .expect("an active Agent Token registers an Agent");

        let identity = AgentIdentity::load(state.path()).expect("the Agent identity was written");
        let agent = api.agent(location_id, &identity.metadata.agent_id).await;
        assert_eq!(agent["status"], "registered");
    }

    let token_id = body["token"]["id"].as_str().expect("the token id");

    api.post(
        &format!("/synthetics/agent-tokens/{token_id}/disable"),
        json!({}),
    )
    .await;

    instructions.assert_refused("a disabled Agent Token").await;
}

async fn a_registered_agent_goes_online(api: &Api<'_>, location_id: &str, state_dir: &Path) {
    let identity = AgentIdentity::load(state_dir).expect("the Agent identity was written");
    let agent_id = identity.metadata.agent_id;

    let running = tokio::spawn(run(RunOptions {
        state_dir: state_dir.to_path_buf(),
        max_concurrent: 2,
        max_browser_concurrent: 0,
    }));

    let deadline = Instant::now() + ONLINE_TIMEOUT;

    // `online` comes from Hello. The feature level only comes from a heartbeat, which the Agent
    // starts after it has checked that the first thing back is a HelloAck.
    loop {
        let agent = api.agent(location_id, &agent_id).await;

        if agent["status"] == "online" && agent["featureLevel"] == FEATURE_LEVEL {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "the Agent is not online after {ONLINE_TIMEOUT:?}: {agent}"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    running.abort();
}
