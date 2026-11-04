// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod assertion;
mod browser;
mod http;
mod network;

pub use assertion::{
    AssertionOperator, AssertionSeverity, Extraction, MonitorAssertion, ValueSource,
    assertion_source_allowed,
};
pub use browser::{BrowserAction, BrowserJourneySpec, BrowserStep, Viewport};
pub use http::{
    ClientCertificate, ClientKey, HeaderValue, HostBinding, HttpJourneySpec, HttpStep, HttpVersion,
};
pub use network::{
    DnsSpec, GrpcCall, GrpcSpec, IcmpSpec, SshAuthentication, SshSpec, TcpSpec, TlsSpec,
};
use serde::{Deserialize, Serialize};

use crate::{
    domain::alerting::incident::Severity,
    shared::{ids::Id, time::TimestampMicros},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorKind {
    Http,
    Tcp,
    Ssh,
    Dns,
    Icmp,
    Tls,
    Grpc,
    Browser,
    Heartbeat,
}

impl MonitorKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Tcp => "tcp",
            Self::Ssh => "ssh",
            Self::Dns => "dns",
            Self::Icmp => "icmp",
            Self::Tls => "tls",
            Self::Grpc => "grpc",
            Self::Browser => "browser",
            Self::Heartbeat => "heartbeat",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "http" => Some(Self::Http),
            "tcp" => Some(Self::Tcp),
            "ssh" => Some(Self::Ssh),
            "dns" => Some(Self::Dns),
            "icmp" => Some(Self::Icmp),
            "tls" => Some(Self::Tls),
            "grpc" => Some(Self::Grpc),
            "browser" => Some(Self::Browser),
            "heartbeat" => Some(Self::Heartbeat),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorLifecycle {
    Draft,
    Active,
    Paused,
    Archived,
}

impl MonitorLifecycle {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Archived => "archived",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "draft" => Some(Self::Draft),
            "active" => Some(Self::Active),
            "paused" => Some(Self::Paused),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MonitorState {
    Healthy,
    Flaky,
    Degraded,
    Failing,
    Unknown,
}

impl MonitorState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Flaky => "flaky",
            Self::Degraded => "degraded",
            Self::Failing => "failing",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "healthy" => Some(Self::Healthy),
            "flaky" => Some(Self::Flaky),
            "degraded" => Some(Self::Degraded),
            "failing" => Some(Self::Failing),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }

    /// 监控项处在这个状态时要不要告警，以及告警的严重度：失败总要告警（error），降级、不稳定看
    /// 监控项的开关（warning），健康不告警；未知什么都不改变（`None`）。状态页自动化与告警事件
    /// 用同一套口径。
    pub fn alert_activation(
        self,
        alert_on_degraded: bool,
        alert_on_flaky: bool,
    ) -> Option<(bool, Severity)> {
        match self {
            Self::Healthy => Some((false, Severity::Info)),
            Self::Flaky => Some((alert_on_flaky, Self::warning_or_info(alert_on_flaky))),
            Self::Degraded => Some((alert_on_degraded, Self::warning_or_info(alert_on_degraded))),
            Self::Failing => Some((true, Severity::Error)),
            Self::Unknown => None,
        }
    }

    const fn warning_or_info(alerting: bool) -> Severity {
        if alerting {
            Severity::Warning
        } else {
            Severity::Info
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum MonitorSchedule {
    Interval {
        every_seconds: u32,
    },
    Cron {
        expression: String,
        timezone: String,
    },
    Heartbeat {
        expected_seconds: u32,
        grace_seconds: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MultiLocationPolicy {
    Any,
    Quorum { required: u32 },
    Majority,
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "configuration", rename_all = "snake_case")]
pub enum MonitorSpec {
    Http(HttpJourneySpec),
    Tcp(TcpSpec),
    Ssh(SshSpec),
    Dns(DnsSpec),
    Icmp(IcmpSpec),
    Tls(TlsSpec),
    Grpc(GrpcSpec),
    Browser(BrowserJourneySpec),
    Heartbeat,
}

impl MonitorSpec {
    pub const fn kind(&self) -> MonitorKind {
        match self {
            Self::Http(_) => MonitorKind::Http,
            Self::Tcp(_) => MonitorKind::Tcp,
            Self::Ssh(_) => MonitorKind::Ssh,
            Self::Dns(_) => MonitorKind::Dns,
            Self::Icmp(_) => MonitorKind::Icmp,
            Self::Tls(_) => MonitorKind::Tls,
            Self::Grpc(_) => MonitorKind::Grpc,
            Self::Browser(_) => MonitorKind::Browser,
            Self::Heartbeat => MonitorKind::Heartbeat,
        }
    }

    /// The lowest Agent feature level that runs this check as written; an older Agent would run
    /// the check without what came later, or not at all.
    ///
    /// - 1 brought ICMP, DNS of every record type through a chosen resolver with DNSSEC and an
    ///   expected response code, and assertions on TCP, DNS and gRPC checks.
    /// - 2 brought to HTTP checks JSONPath and JSON Schema, the size and timing sources, HTTP/2
    ///   only, client certificates, a chosen DNS server and host bindings. HTTP/1.1 only needs no
    ///   level: it is all an older Agent speaks.
    pub fn required_feature_level(&self) -> u32 {
        match self {
            Self::Http(spec) if http_needs_level_two(spec) => 2,
            Self::Icmp(_) => 1,
            Self::Dns(spec)
                if !matches!(spec.record_type.to_ascii_uppercase().as_str(), "A" | "AAAA")
                    || spec
                        .resolver
                        .as_deref()
                        .is_some_and(|resolver| !resolver.trim().is_empty())
                    || spec.require_dnssec
                    || spec
                        .expected_rcode
                        .as_deref()
                        .is_some_and(|code| !code.trim().is_empty())
                    || !spec.assertions.is_empty() =>
            {
                1
            }
            Self::Tcp(spec) if !spec.assertions.is_empty() => 1,
            Self::Grpc(spec) if !spec.assertions.is_empty() => 1,
            _ => 0,
        }
    }
}

fn http_needs_level_two(spec: &HttpJourneySpec) -> bool {
    let late_source = |source: &str| {
        source.starts_with("json:") || source == "size_bytes" || source.starts_with("timing.")
    };

    spec.http_version == HttpVersion::Http2
        || spec.client_certificate.is_some()
        || spec
            .dns_server
            .as_deref()
            .is_some_and(|server| !server.trim().is_empty())
        || !spec.hosts.is_empty()
        || spec.steps.iter().any(|step| {
            step.assertions.iter().any(|assertion| {
                late_source(&assertion.source)
                    || matches!(assertion.operator, AssertionOperator::JsonSchema { .. })
            }) || step
                .extractions
                .iter()
                .any(|extraction| late_source(&extraction.source))
        })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyntheticMonitor {
    pub id: Id,
    pub organization_id: Id,
    pub name: String,
    pub description: String,
    pub kind: MonitorKind,
    pub lifecycle: MonitorLifecycle,
    pub state: MonitorState,
    pub team_id: Option<Id>,
    pub tags: Vec<String>,
    pub active_revision_id: Option<Id>,
    pub draft_revision_id: Option<Id>,
    pub next_due_at: Option<TimestampMicros>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub updated_at: TimestampMicros,
    pub archived_at: Option<TimestampMicros>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MonitorRevision {
    pub id: Id,
    pub organization_id: Id,
    pub monitor_id: Id,
    pub number: u32,
    pub spec: MonitorSpec,
    pub schedule: MonitorSchedule,
    pub timeout_millis: u32,
    pub max_retries: u8,
    pub consecutive_failures: u32,
    pub consecutive_recoveries: u32,
    pub freshness_seconds: u32,
    pub location_policy: MultiLocationPolicy,
    pub location_ids: Vec<Id>,
    pub escalation_policy_id: Option<Id>,
    pub alert_on_degraded: bool,
    pub alert_on_flaky: bool,
    /// Whether an incident opens when the check stops getting results: when as many locations as
    /// the location policy needs missed as many runs in a row as fail a location.
    pub alert_on_no_data: bool,
    /// Whether HTTP and gRPC requests carry a W3C trace context, so that the traces of the
    /// services they reach lead back to the run.
    pub inject_trace_context: bool,
    /// Whether a run that cannot reach its target traces the route to the address it tried.
    pub traceroute_on_failure: bool,
    pub last_test_result_id: Option<Id>,
    pub last_test_passed_at: Option<TimestampMicros>,
    pub created_by: Id,
    pub created_at: TimestampMicros,
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveMonitorRevision {
    pub monitor: SyntheticMonitor,
    pub revision: MonitorRevision,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failing_monitor_always_alerts_and_the_others_follow_their_switches() {
        assert_eq!(
            MonitorState::Failing.alert_activation(false, false),
            Some((true, Severity::Error))
        );
        assert_eq!(
            MonitorState::Degraded.alert_activation(true, false),
            Some((true, Severity::Warning))
        );
        assert_eq!(
            MonitorState::Degraded.alert_activation(false, true),
            Some((false, Severity::Info))
        );
        assert_eq!(
            MonitorState::Flaky.alert_activation(false, true),
            Some((true, Severity::Warning))
        );
        assert_eq!(
            MonitorState::Flaky.alert_activation(true, false),
            Some((false, Severity::Info))
        );
        assert_eq!(
            MonitorState::Healthy.alert_activation(true, true),
            Some((false, Severity::Info))
        );
        assert_eq!(MonitorState::Unknown.alert_activation(true, true), None);
    }

    /// An HTTP check that asks for a 200, with `change` made to its configuration.
    fn http_check(change: fn(&mut serde_json::Value)) -> MonitorSpec {
        let mut spec = serde_json::json!({
            "kind": "http",
            "configuration": {
                "steps": [{
                    "id": "step",
                    "name": "Request",
                    "method": "GET",
                    "url": { "source": "literal", "value": "https://api.example.com/health" },
                    "assertions": [{
                        "id": "status",
                        "name": "Status",
                        "source": "status",
                        "severity": "critical",
                        "operator": { "operator": "equals", "expected": "200" }
                    }]
                }],
                "followRedirects": true,
                "maxRedirects": 5,
                "verifyTls": true
            }
        });

        change(&mut spec["configuration"]);
        serde_json::from_value(spec).expect("an HTTP check")
    }

    #[test]
    fn an_http_check_waits_for_an_agent_that_knows_what_it_uses() {
        assert_eq!(http_check(|_| {}).required_feature_level(), 0);
        assert_eq!(
            http_check(|spec| spec["httpVersion"] = "http1".into()).required_feature_level(),
            0
        );

        // An Agent older than level 2 would run each of these without the setting, against
        // another address or protocol, or with no certificate to show.
        let level_two: [fn(&mut serde_json::Value); 8] = [
            |spec| spec["httpVersion"] = "http2".into(),
            |spec| spec["dnsServer"] = "1.1.1.1:53".into(),
            |spec| {
                spec["hosts"] =
                    serde_json::json!([{ "name": "api.example.com", "addresses": ["192.0.2.10"] }]);
            },
            |spec| {
                let key = serde_json::json!({ "state": "new", "pem": "KEY" });
                spec["clientCertificate"] =
                    serde_json::json!({ "certificate": "CERTIFICATE", "privateKey": key });
            },
            |spec| spec["steps"][0]["assertions"][0]["source"] = "json:$.status".into(),
            |spec| spec["steps"][0]["assertions"][0]["source"] = "timing.tls_ms".into(),
            |spec| {
                let operator = &mut spec["steps"][0]["assertions"][0]["operator"];
                *operator = serde_json::json!({ "operator": "json_schema", "schema": {} });
            },
            |spec| {
                spec["steps"][0]["extractions"] = serde_json::json!([{
                    "variable": "size",
                    "source": "size_bytes",
                    "expression": "",
                    "required": false
                }]);
            },
        ];

        for change in level_two {
            assert_eq!(http_check(change).required_feature_level(), 2);
        }
    }
}
