// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Context as _, Result, anyhow};
use regex::Regex;
use russh::{client, keys::ssh_key};
use tokio::{
    io::{AsyncRead, AsyncReadExt as _, AsyncWriteExt as _},
    net::TcpStream,
};

use super::{AttemptOutcome, ExecutionContext, now_micros, security::EgressGuard};
use crate::protocol::v1::{self as wire, ProbeOutcome, SshSpec, ssh_spec::Authentication};

mod session;

use session::{authenticate, execute_command};

const CLIENT_IDENTIFICATION: &str = "SSH-2.0-MoleSignal\r\n";
const IDENTIFICATION_MAX_BYTES: usize = 255;
const PRE_BANNER_MAX_LINES: usize = 50;

#[derive(Clone)]
struct HostKeyVerifier {
    expected: String,
    observed: Arc<std::sync::Mutex<Option<String>>>,
}

impl client::Handler for HostKeyVerifier {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key
            .fingerprint(ssh_key::HashAlg::Sha256)
            .to_string();
        *self.observed.lock().expect("SSH fingerprint lock poisoned") = Some(fingerprint.clone());
        Ok(fingerprint == self.expected)
    }
}

pub(super) async fn execute(
    task: &wire::ProbeTask,
    spec: &SshSpec,
    context: &ExecutionContext,
) -> Result<AttemptOutcome> {
    let host = context.resolve(
        spec.host
            .as_ref()
            .ok_or_else(|| anyhow!("SSH host is required"))?,
    )?;
    let port = u16::try_from(spec.port).context("SSH port is out of range")?;
    let address = EgressGuard::new(task.egress_policy.clone())?
        .resolve(&host, port)
        .await?
        .remove(0);
    let timeout = Duration::from_millis(u64::from(task.timeout_millis.max(1)));
    tokio::time::timeout(timeout, execute_inner(address, spec, context))
        .await
        .context("SSH check timed out")?
}

async fn execute_inner(
    address: std::net::SocketAddr,
    spec: &SshSpec,
    context: &ExecutionContext,
) -> Result<AttemptOutcome> {
    let started_at = now_micros();
    let identification = exchange_identification(address).await?;
    let identification_finished = now_micros();
    let Some(identification) = identification else {
        return Ok(failure(
            "ssh_identification_missing",
            "SSH server did not send a valid protocol identification",
            HashMap::from([("remote_address".into(), address.to_string())]),
            vec![evidence(
                "identification",
                started_at,
                identification_finished,
                ProbeOutcome::Failing,
                "ssh_identification_missing",
                "SSH server did not send a valid protocol identification",
                HashMap::new(),
            )],
            Vec::new(),
        ));
    };
    let identification_matches = spec
        .expected_identification_regex
        .as_ref()
        .map(|pattern| Regex::new(pattern).map(|regex| regex.is_match(&identification)))
        .transpose()?
        .unwrap_or(true);
    let identification_evidence = evidence(
        "identification",
        started_at,
        identification_finished,
        if identification_matches {
            ProbeOutcome::Healthy
        } else {
            ProbeOutcome::Failing
        },
        if identification_matches {
            ""
        } else {
            "ssh_identification_mismatch"
        },
        if identification_matches {
            ""
        } else {
            "SSH server identification did not match the expected pattern"
        },
        HashMap::from([("server_identification".into(), identification.clone())]),
    );
    let base_metadata = HashMap::from([
        ("remote_address".into(), address.to_string()),
        ("server_identification".into(), identification.clone()),
    ]);
    if !identification_matches {
        return Ok(failure(
            "ssh_identification_mismatch",
            "SSH server identification did not match the expected pattern",
            base_metadata,
            vec![identification_evidence],
            context.redact(&identification).into_bytes(),
        ));
    }
    let Some(authentication) = &spec.authentication else {
        return Ok(success(base_metadata, vec![identification_evidence]));
    };
    authenticated_check(
        address,
        spec,
        authentication,
        context,
        base_metadata,
        identification_evidence,
    )
    .await
}

async fn authenticated_check(
    address: std::net::SocketAddr,
    spec: &SshSpec,
    authentication: &Authentication,
    context: &ExecutionContext,
    mut metadata: HashMap<String, String>,
    identification_evidence: wire::StepEvidence,
) -> Result<AttemptOutcome> {
    let expected_fingerprint = spec
        .expected_host_key_sha256
        .clone()
        .ok_or_else(|| anyhow!("SSH authentication requires a host key fingerprint"))?;
    let observed = Arc::new(std::sync::Mutex::new(None));
    let verifier = HostKeyVerifier {
        expected: expected_fingerprint.clone(),
        observed: observed.clone(),
    };
    let stream = TcpStream::connect(address).await?;
    let config = Arc::new(client::Config {
        inactivity_timeout: None,
        ..Default::default()
    });
    let auth_started = now_micros();
    let connected = client::connect_stream(config, stream, verifier).await;
    let observed_fingerprint = observed
        .lock()
        .expect("SSH fingerprint lock poisoned")
        .clone();
    if let Some(fingerprint) = &observed_fingerprint {
        metadata.insert("host_key_sha256".into(), fingerprint.clone());
    }
    let mut session = match connected {
        Ok(session) => session,
        Err(_)
            if observed_fingerprint
                .as_deref()
                .is_some_and(|value| value != expected_fingerprint) =>
        {
            let finished = now_micros();
            return Ok(failure(
                "ssh_host_key_mismatch",
                "SSH host key fingerprint did not match",
                metadata,
                vec![
                    identification_evidence,
                    evidence(
                        "authentication",
                        auth_started,
                        finished,
                        ProbeOutcome::Failing,
                        "ssh_host_key_mismatch",
                        "SSH host key fingerprint did not match",
                        HashMap::from([("expected_host_key_sha256".into(), expected_fingerprint)]),
                    ),
                ],
                Vec::new(),
            ));
        }
        Err(error) => return Err(error),
    };
    let (method, authenticated) = authenticate(&mut session, authentication, context).await?;
    let auth_finished = now_micros();
    metadata.insert("auth_method".into(), method.to_string());
    let auth_evidence = evidence(
        "authentication",
        auth_started,
        auth_finished,
        if authenticated {
            ProbeOutcome::Healthy
        } else {
            ProbeOutcome::Failing
        },
        if authenticated {
            ""
        } else {
            "ssh_authentication_failed"
        },
        if authenticated {
            ""
        } else {
            "SSH server rejected the configured credentials"
        },
        HashMap::from([("auth_method".into(), method.to_string())]),
    );
    if !authenticated {
        return Ok(failure(
            "ssh_authentication_failed",
            "SSH server rejected the configured credentials",
            metadata,
            vec![identification_evidence, auth_evidence],
            Vec::new(),
        ));
    }
    let Some(command) = &spec.command else {
        return Ok(success(
            metadata,
            vec![identification_evidence, auth_evidence],
        ));
    };
    let command = context.resolve(command)?;
    let command_started = now_micros();
    let result = execute_command(&mut session, &command).await?;
    let command_finished = now_micros();
    let output = String::from_utf8_lossy(&result.output);
    let output_matches = spec
        .expected_output_regex
        .as_ref()
        .map(|pattern| Regex::new(pattern).map(|regex| regex.is_match(&output)))
        .transpose()?
        .unwrap_or(true);
    let expected_status = spec.expected_exit_status.unwrap_or(0);
    let status_matches = result.exit_status == Some(expected_status);
    let passed = output_matches && status_matches;
    metadata.insert(
        "command_exit_status".into(),
        result
            .exit_status
            .map_or_else(|| "missing".into(), |status| status.to_string()),
    );
    metadata.insert("output_truncated".into(), result.truncated.to_string());
    let command_evidence = evidence(
        "command",
        command_started,
        command_finished,
        if passed {
            ProbeOutcome::Healthy
        } else {
            ProbeOutcome::Failing
        },
        if passed { "" } else { "ssh_command_failed" },
        if passed {
            ""
        } else {
            "SSH command output or exit status did not match expectations"
        },
        HashMap::from([
            ("expected_exit_status".into(), expected_status.to_string()),
            (
                "actual_exit_status".into(),
                result
                    .exit_status
                    .map_or_else(|| "missing".into(), |status| status.to_string()),
            ),
            ("output_truncated".into(), result.truncated.to_string()),
        ]),
    );
    let evidence = vec![identification_evidence, auth_evidence, command_evidence];
    if passed {
        Ok(success(metadata, evidence))
    } else {
        Ok(failure(
            "ssh_command_failed",
            "SSH command output or exit status did not match expectations",
            metadata,
            evidence,
            context.redact(&output).into_bytes(),
        ))
    }
}

async fn exchange_identification(address: std::net::SocketAddr) -> Result<Option<String>> {
    let mut stream = TcpStream::connect(address).await?;
    stream.write_all(CLIENT_IDENTIFICATION.as_bytes()).await?;
    stream.flush().await?;
    read_identification(&mut stream).await
}

async fn read_identification(stream: &mut (impl AsyncRead + Unpin)) -> Result<Option<String>> {
    let mut line = Vec::with_capacity(IDENTIFICATION_MAX_BYTES);
    let mut byte = [0_u8; 1];
    for _ in 0..PRE_BANNER_MAX_LINES {
        line.clear();
        loop {
            if line.len() == IDENTIFICATION_MAX_BYTES || stream.read(&mut byte).await? == 0 {
                return Ok(None);
            }
            line.push(byte[0]);
            if byte[0] == b'\n' {
                break;
            }
        }
        let identification = line.strip_suffix(b"\n").unwrap_or(&line);
        let identification = identification.strip_suffix(b"\r").unwrap_or(identification);
        let supported =
            identification.starts_with(b"SSH-2.0-") || identification.starts_with(b"SSH-1.99-");
        let printable = identification
            .iter()
            .all(|byte| byte.is_ascii_graphic() || *byte == b' ');
        if supported && printable {
            return Ok(Some(String::from_utf8(identification.to_vec())?));
        }
        if identification.starts_with(b"SSH-") {
            return Ok(None);
        }
    }
    Ok(None)
}

fn success(metadata: HashMap<String, String>, evidence: Vec<wire::StepEvidence>) -> AttemptOutcome {
    AttemptOutcome {
        outcome: ProbeOutcome::Healthy,
        assertions: Vec::new(),
        response_excerpt: Vec::new(),
        error_category: String::new(),
        error_message: String::new(),
        metadata,
        evidence,
        artifacts: Vec::new(),
    }
}

fn failure(
    category: &str,
    message: &str,
    metadata: HashMap<String, String>,
    evidence: Vec<wire::StepEvidence>,
    response_excerpt: Vec<u8>,
) -> AttemptOutcome {
    AttemptOutcome {
        outcome: ProbeOutcome::Failing,
        assertions: Vec::new(),
        response_excerpt,
        error_category: category.into(),
        error_message: message.into(),
        metadata,
        evidence,
        artifacts: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn evidence(
    id: &str,
    started_at_micros: i64,
    finished_at_micros: i64,
    outcome: ProbeOutcome,
    error_category: &str,
    error_message: &str,
    metadata: HashMap<String, String>,
) -> wire::StepEvidence {
    wire::StepEvidence {
        step_id: format!("ssh-{id}"),
        name: match id {
            "identification" => "SSH identification",
            "authentication" => "SSH authentication",
            "command" => "SSH command",
            _ => "SSH step",
        }
        .into(),
        action: id.into(),
        started_at_micros,
        finished_at_micros,
        outcome: outcome as i32,
        error_category: error_category.into(),
        error_message: error_message.into(),
        metadata,
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncWriteExt as _, duplex};

    use super::read_identification;

    async fn parse(input: &[u8]) -> Option<String> {
        let (mut reader, mut writer) = duplex(1024);
        writer.write_all(input).await.unwrap();
        drop(writer);
        read_identification(&mut reader).await.unwrap()
    }

    #[tokio::test]
    async fn accepts_identification_after_pre_banner() {
        assert_eq!(
            parse(b"Authorized access only\r\nSSH-2.0-OpenSSH_9.9 Ubuntu-3\r\n")
                .await
                .as_deref(),
            Some("SSH-2.0-OpenSSH_9.9 Ubuntu-3")
        );
    }

    #[tokio::test]
    async fn rejects_legacy_protocol() {
        assert_eq!(parse(b"SSH-1.5-legacy\r\n").await, None);
    }
}
