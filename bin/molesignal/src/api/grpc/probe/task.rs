// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use base64::Engine as _;

use crate::{
    app::synthetics::{ResolvedProbeSecret, artifact_targets},
    domain::synthetics::{
        AssertionOperator, AssertionSeverity, BrowserAction, EgressPolicy, GrpcCall, HeaderValue,
        MonitorAssertion, MonitorSpec, ProbeCapability, ProbeLocation, ProbeTask,
        SshAuthentication, ValueSource,
    },
    protocol::probe::v1 as wire,
    shared::{Error, Result},
};

pub(super) fn capabilities_from_wire(values: &[i32]) -> Result<Vec<ProbeCapability>> {
    values
        .iter()
        .map(|value| match wire::Capability::try_from(*value).ok() {
            Some(wire::Capability::Http) => Ok(ProbeCapability::Http),
            Some(wire::Capability::Tcp) => Ok(ProbeCapability::Tcp),
            Some(wire::Capability::Ssh) => Ok(ProbeCapability::Ssh),
            Some(wire::Capability::Dns) => Ok(ProbeCapability::Dns),
            Some(wire::Capability::Icmp) => Ok(ProbeCapability::Icmp),
            Some(wire::Capability::Tls) => Ok(ProbeCapability::Tls),
            Some(wire::Capability::Grpc) => Ok(ProbeCapability::Grpc),
            Some(wire::Capability::Browser) => Ok(ProbeCapability::Browser),
            _ => Err(Error::invalid("unsupported Probe capability")),
        })
        .collect()
}

pub(crate) fn task_to_wire(
    task: ProbeTask,
    lease_token: String,
    location: ProbeLocation,
    secrets: Vec<ResolvedProbeSecret>,
    artifact_base_url: Option<&str>,
) -> Result<wire::ProbeTask> {
    let spec = spec_to_wire(&task.spec)?;
    let artifact_uploads = artifact_base_url
        .map(|base_url| {
            artifact_targets(&task, &lease_token)
                .into_iter()
                .map(|target| wire::ArtifactUploadTarget {
                    artifact_id: target.id.clone(),
                    kind: target.kind,
                    upload_url: format!(
                        "{base_url}/api/v1/synthetics/artifacts/{}/{}",
                        task.id, target.id
                    ),
                    upload_headers: [
                        ("authorization".into(), format!("Bearer {lease_token}")),
                        ("content-type".into(), target.content_type),
                    ]
                    .into_iter()
                    .collect(),
                    expires_at_micros: target.expires_at.0,
                    max_bytes: target.max_bytes,
                    name: target.name,
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(wire::ProbeTask {
        task_id: task.id.0,
        organization_id: task.organization_id.0,
        monitor_id: task.monitor_id.0,
        monitor_revision_id: task.monitor_revision_id.0,
        location_id: task.location_id.0,
        lease_token,
        leased_until_micros: task.leased_until.map_or(0, |value| value.0),
        scheduled_at_micros: task.scheduled_at.0,
        deadline_micros: task.deadline_at.0,
        timeout_millis: task.timeout_millis,
        max_attempts: u32::from(task.max_attempts),
        egress_policy: Some(egress_to_wire(location.egress_policy)),
        secrets: secrets
            .into_iter()
            .map(|secret| wire::ResolvedSecret {
                reference: secret.reference,
                secret_id: secret.secret_id.0,
                version: secret.version,
                value: secret.material.0.into(),
            })
            .collect(),
        test_run: task.is_test,
        artifact_uploads,
        spec: Some(spec),
    })
}

fn egress_to_wire(policy: EgressPolicy) -> wire::EgressPolicy {
    wire::EgressPolicy {
        allowed_cidrs: policy.allowed_cidrs,
        denied_cidrs: policy.denied_cidrs,
        allowed_domains: policy.allowed_domains,
        denied_domains: policy.denied_domains,
        allowed_ports: policy.allowed_ports.into_iter().map(u32::from).collect(),
        allow_private_networks: policy.allow_private_networks,
        allow_loopback: policy.allow_loopback,
    }
}

fn spec_to_wire(spec: &MonitorSpec) -> Result<wire::probe_task::Spec> {
    use wire::probe_task::Spec;
    Ok(match spec {
        MonitorSpec::Http(value) => Spec::Http(wire::HttpJourneySpec {
            steps: value
                .steps
                .iter()
                .map(|step| wire::HttpStep {
                    id: step.id.0.clone(),
                    name: step.name.clone(),
                    method: step.method.clone(),
                    url: Some(value_to_wire(&step.url)),
                    headers: headers_to_wire(&step.headers),
                    query: headers_to_wire(&step.query),
                    body: step.body.as_ref().map(value_to_wire),
                    extractions: step
                        .extractions
                        .iter()
                        .map(|item| wire::Extraction {
                            variable: item.variable.clone(),
                            source: item.source.clone(),
                            expression: item.expression.clone(),
                            required: item.required,
                        })
                        .collect(),
                    assertions: step.assertions.iter().map(assertion_to_wire).collect(),
                })
                .collect(),
            follow_redirects: value.follow_redirects,
            max_redirects: u32::from(value.max_redirects),
            verify_tls: value.verify_tls,
        }),
        MonitorSpec::Tcp(value) => Spec::Tcp(wire::TcpSpec {
            host: Some(value_to_wire(&value.host)),
            port: u32::from(value.port),
            use_tls: value.use_tls,
            server_name: value.server_name.clone().unwrap_or_default(),
            send: value.send.as_ref().map(value_to_wire),
            expect_regex: value.expect_regex.clone(),
            assertions: value.assertions.iter().map(assertion_to_wire).collect(),
        }),
        MonitorSpec::Ssh(value) => Spec::Ssh(wire::SshSpec {
            host: Some(value_to_wire(&value.host)),
            port: u32::from(value.port),
            expected_identification_regex: value.expected_identification_regex.clone(),
            authentication: value.authentication.as_ref().map(
                |authentication| match authentication {
                    SshAuthentication::Password { username, password } => {
                        wire::ssh_spec::Authentication::Password(wire::SshPasswordAuthentication {
                            username: Some(value_to_wire(username)),
                            password: Some(value_to_wire(password)),
                        })
                    }
                    SshAuthentication::PublicKey {
                        username,
                        private_key,
                        passphrase,
                    } => wire::ssh_spec::Authentication::PublicKey(
                        wire::SshPublicKeyAuthentication {
                            username: Some(value_to_wire(username)),
                            private_key: Some(value_to_wire(private_key)),
                            passphrase: passphrase.as_ref().map(value_to_wire),
                        },
                    ),
                },
            ),
            command: value.command.as_ref().map(value_to_wire),
            expected_output_regex: value.expected_output_regex.clone(),
            expected_exit_status: value.expected_exit_status,
            expected_host_key_sha256: value.expected_host_key_sha256.clone(),
        }),
        MonitorSpec::Dns(value) => Spec::Dns(wire::DnsSpec {
            name: value.name.clone(),
            record_type: value.record_type.clone(),
            resolver: value.resolver.clone(),
            require_dnssec: value.require_dnssec,
            expected_values: value.expected_values.clone(),
            expected_rcode: value.expected_rcode.clone(),
            assertions: value.assertions.iter().map(assertion_to_wire).collect(),
        }),
        MonitorSpec::Icmp(value) => Spec::Icmp(wire::IcmpSpec {
            host: value.host.clone(),
            count: u32::from(value.count),
            interval_millis: value.interval_millis,
            max_packet_loss_ratio: value.max_packet_loss_ratio,
            max_mean_rtt_millis: value.max_mean_rtt_millis,
        }),
        MonitorSpec::Tls(value) => Spec::Tls(wire::TlsSpec {
            host: value.host.clone(),
            port: u32::from(value.port),
            server_name: value.server_name.clone(),
            minimum_days_remaining: value.minimum_days_remaining,
            expected_sans: value.expected_sans.clone(),
            expected_issuer_regex: value.expected_issuer_regex.clone(),
            minimum_protocol: value.minimum_protocol.clone(),
        }),
        MonitorSpec::Grpc(value) => Spec::Grpc(wire::GrpcSpec {
            endpoint: value.endpoint.clone(),
            use_tls: value.use_tls,
            server_name: value.server_name.clone(),
            metadata: headers_to_wire(&value.metadata),
            assertions: value.assertions.iter().map(assertion_to_wire).collect(),
            call: Some(match &value.call {
                GrpcCall::Health { service } => {
                    wire::grpc_spec::Call::Health(wire::GrpcHealthCall {
                        service: service.clone(),
                    })
                }
                GrpcCall::Unary {
                    service,
                    method,
                    descriptor_set_base64,
                    use_reflection,
                    request_json,
                } => wire::grpc_spec::Call::Unary(wire::GrpcUnaryCall {
                    service: service.clone(),
                    method: method.clone(),
                    descriptor_set: descriptor_set_base64
                        .as_ref()
                        .map(|encoded| {
                            base64::engine::general_purpose::STANDARD
                                .decode(encoded)
                                .map(Into::into)
                                .map_err(|_| Error::invalid("invalid gRPC descriptor set"))
                        })
                        .transpose()?,
                    use_reflection: *use_reflection,
                    request_json: request_json.clone(),
                }),
            }),
        }),
        MonitorSpec::Browser(value) => Spec::Browser(wire::BrowserJourneySpec {
            steps: value
                .steps
                .iter()
                .map(|step| wire::BrowserStep {
                    id: step.id.0.clone(),
                    name: step.name.clone(),
                    action: Some(browser_action_to_wire(&step.action)),
                })
                .collect(),
            viewport: Some(wire::Viewport {
                width: value.viewport.width,
                height: value.viewport.height,
            }),
            user_agent: value.user_agent.clone().unwrap_or_default(),
            capture_screenshot_on_failure: value.capture_screenshot_on_failure,
            capture_har_on_failure: value.capture_har_on_failure,
            capture_trace_on_failure: value.capture_trace_on_failure,
        }),
        MonitorSpec::Heartbeat => {
            return Err(Error::invalid(
                "Heartbeat Monitors do not create Probe tasks",
            ));
        }
    })
}

fn browser_action_to_wire(action: &BrowserAction) -> wire::browser_step::Action {
    use wire::browser_step::Action;
    match action {
        BrowserAction::Navigate { url, wait_until } => Action::Navigate(wire::BrowserNavigate {
            url: Some(value_to_wire(url)),
            wait_until: wait_until.clone(),
        }),
        BrowserAction::Click { selector } => Action::Click(wire::BrowserClick {
            selector: selector.clone(),
        }),
        BrowserAction::Fill { selector, value } => Action::Fill(wire::BrowserFill {
            selector: selector.clone(),
            value: Some(value_to_wire(value)),
        }),
        BrowserAction::Select { selector, value } => Action::Select(wire::BrowserSelect {
            selector: selector.clone(),
            value: Some(value_to_wire(value)),
        }),
        BrowserAction::WaitDuration { duration_millis } => Action::Wait(wire::BrowserWait {
            condition: Some(wire::browser_wait::Condition::DurationMillis(
                *duration_millis,
            )),
        }),
        BrowserAction::WaitSelector { selector } => Action::Wait(wire::BrowserWait {
            condition: Some(wire::browser_wait::Condition::Selector(selector.clone())),
        }),
        BrowserAction::WaitExpression { expression } => Action::Wait(wire::BrowserWait {
            condition: Some(wire::browser_wait::Condition::PageExpression(
                expression.clone(),
            )),
        }),
        BrowserAction::Extract {
            variable,
            selector,
            source,
        } => Action::Extract(wire::BrowserExtract {
            variable: variable.clone(),
            selector: selector.clone(),
            source: source.clone(),
        }),
        BrowserAction::Assert {
            assertion,
            selector,
            page_expression,
        } => Action::Assertion(wire::BrowserAssert {
            assertion: Some(assertion_to_wire(assertion)),
            selector: selector.clone(),
            page_expression: page_expression.clone(),
        }),
        BrowserAction::Screenshot { name, full_page } => {
            Action::Screenshot(wire::BrowserScreenshot {
                name: name.clone(),
                full_page: *full_page,
            })
        }
    }
}

fn headers_to_wire(values: &[HeaderValue]) -> Vec<wire::HeaderValue> {
    values
        .iter()
        .map(|value| wire::HeaderValue {
            name: value.name.clone(),
            value: Some(value_to_wire(&value.value)),
        })
        .collect()
}

fn value_to_wire(value: &ValueSource) -> wire::Value {
    use wire::value::Source;
    wire::Value {
        source: Some(match value {
            ValueSource::Literal { value } => Source::Literal(value.clone()),
            ValueSource::Variable { name } => Source::VariableReference(name.clone()),
            ValueSource::Secret { reference, .. } => Source::SecretReference(reference.clone()),
        }),
    }
}

fn assertion_to_wire(assertion: &MonitorAssertion) -> wire::Assertion {
    let (operator, expected) = match &assertion.operator {
        AssertionOperator::Equals { expected } => ("equals", Some(expected.clone())),
        AssertionOperator::NotEquals { expected } => ("not_equals", Some(expected.clone())),
        AssertionOperator::Contains { expected } => ("contains", Some(expected.clone())),
        AssertionOperator::NotContains { expected } => ("not_contains", Some(expected.clone())),
        AssertionOperator::Matches { pattern } => ("matches", Some(pattern.clone())),
        AssertionOperator::GreaterThan { expected } => ("greater_than", Some(expected.to_string())),
        AssertionOperator::LessThan { expected } => ("less_than", Some(expected.to_string())),
        AssertionOperator::JsonSchema { schema } => ("json_schema", Some(schema.to_string())),
        AssertionOperator::Exists => ("exists", None),
    };
    wire::Assertion {
        id: assertion.id.0.clone(),
        name: assertion.name.clone(),
        severity: match assertion.severity {
            AssertionSeverity::Warning => wire::AssertionSeverity::Warning as i32,
            AssertionSeverity::Critical => wire::AssertionSeverity::Critical as i32,
        },
        source: assertion.source.clone(),
        operator: operator.to_string(),
        expected: expected.map(|value| wire::Value {
            source: Some(wire::value::Source::Literal(value)),
        }),
    }
}
