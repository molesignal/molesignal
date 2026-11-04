// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! A traceroute to the address a failed run could not reach: which hops answer on the way there,
//! how fast, and how many of the probes each one loses.

use std::{collections::HashMap, net::IpAddr, time::Duration};

use serde_json::{Value, json};
use trippy_core::{Builder, PrivilegeMode, Protocol, State};

use crate::{
    executor::now_micros,
    protocol::v1::{self as wire, ProbeOutcome},
};

const MAX_HOPS: u8 = 30;

/// Probes sent to every hop, one a round.
const ROUNDS: usize = 4;

/// How long a round waits for the hops that have not answered.
const ROUND: Duration = Duration::from_secs(2);

/// All the time a traceroute has, however its rounds go.
const LIMIT: Duration = Duration::from_secs(12);

/// The most the hops can take in the evidence, which bounds each of its values.
const MAX_HOPS_TEXT: usize = 4_000;

/// Whether a failure is one a traceroute tells something about: the target did not answer, or
/// the connection to it broke off.
pub(in crate::executor) fn worth_tracing(category: &str) -> bool {
    matches!(
        category,
        "connect_failed"
            | "timeout"
            | "tls_failed"
            | "connection_lost"
            | "http_failed"
            | "grpc_failed"
            | "icmp_no_reply"
            | "icmp_packet_loss"
    )
}

/// Traces the route to `target`, as a step of the attempt. A traceroute the Agent may not run
/// says why instead.
pub(in crate::executor) async fn trace(target: IpAddr) -> wire::StepEvidence {
    let started = now_micros();
    let running = tokio::task::spawn_blocking(move || run(target));
    let mut metadata = HashMap::from([("target".to_string(), target.to_string())]);

    let failed = match tokio::time::timeout(LIMIT, running).await {
        Ok(Ok(Ok(traced))) => {
            metadata.extend(traced);
            None
        }
        Ok(Ok(Err(why))) => Some(why),
        Ok(Err(_)) => Some("the traceroute stopped unexpectedly".to_string()),
        Err(_) => Some(format!(
            "the traceroute took longer than {} s",
            LIMIT.as_secs()
        )),
    };

    wire::StepEvidence {
        step_id: "traceroute".into(),
        name: format!("Traceroute to {target}"),
        action: "traceroute".into(),
        started_at_micros: started,
        finished_at_micros: now_micros(),
        outcome: ProbeOutcome::Unknown as i32,
        error_category: failed
            .as_ref()
            .map(|_| "traceroute_unavailable".to_string())
            .unwrap_or_default(),
        error_message: failed.unwrap_or_default(),
        metadata,
    }
}

/// Traces with raw sockets when the process may open them (root, or `CAP_NET_RAW`), else with
/// the ICMP sockets an unprivileged process may open: on macOS, and on Linux within
/// `net.ipv4.ping_group_range`.
fn run(target: IpAddr) -> Result<HashMap<String, String>, String> {
    let mut refusals = Vec::new();

    for mode in [PrivilegeMode::Privileged, PrivilegeMode::Unprivileged] {
        // The tracer never drops privileges: they are the whole process's, not its own.
        let tracer = Builder::new(target)
            .privilege_mode(mode)
            .protocol(Protocol::Icmp)
            .trace_identifier(rand::random())
            .max_ttl(MAX_HOPS)
            .max_rounds(Some(ROUNDS))
            .max_round_duration(ROUND)
            .drop_privileges(false)
            .build()
            .map_err(|error| format!("cannot set the traceroute up: {error}"))?;

        match tracer.run() {
            Ok(()) => return Ok(evidence(&tracer.snapshot(), target, mode)),
            Err(error) => refusals.push(format!("{}: {error}", mode_name(mode))),
        }
    }

    Err(format!(
        "this Agent may not send traceroute probes ({}); give it CAP_NET_RAW, or allow its \
         group in net.ipv4.ping_group_range",
        refusals.join("; ")
    ))
}

fn mode_name(mode: PrivilegeMode) -> &'static str {
    match mode {
        PrivilegeMode::Privileged => "privileged",
        PrivilegeMode::Unprivileged => "unprivileged",
    }
}

/// The hops as evidence. Each is `[ttl, address, addresses, sent, received, best, average,
/// worst]`: the address that answered most often (null when none did), how many answered at
/// that distance, and the round trip times in milliseconds.
fn evidence(state: &State, target: IpAddr, mode: PrivilegeMode) -> HashMap<String, String> {
    let mut hops = state.hops().iter().map(hop).collect::<Vec<_>>();
    let reached = state
        .hops()
        .iter()
        .any(|hop| hop.addrs().any(|address| *address == target));
    let mut text = Value::Array(hops.clone()).to_string();

    // Thirty hops fit with room to spare; a longer list loses its far end.
    while text.len() > MAX_HOPS_TEXT && hops.pop().is_some() {
        text = Value::Array(hops.clone()).to_string();
    }

    HashMap::from([
        ("hops".to_string(), text),
        ("reached".to_string(), reached.to_string()),
        ("mode".to_string(), mode_name(mode).to_string()),
        ("rounds".to_string(), ROUNDS.to_string()),
    ])
}

fn hop(hop: &trippy_core::Hop) -> Value {
    let address = hop
        .addrs_with_counts()
        .max_by_key(|(_, count)| **count)
        .map(|(address, _)| address.to_string());

    let answered = hop.total_recv() > 0;
    let millis = |value: f64| (value * 10.0).round() / 10.0;

    json!([
        hop.ttl(),
        address,
        hop.addr_count(),
        hop.total_sent(),
        hop.total_recv(),
        hop.best_ms().map(millis),
        answered.then(|| millis(hop.avg_ms())),
        hop.worst_ms().map(millis),
    ])
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use super::{trace, worth_tracing};

    #[test]
    fn only_a_target_that_did_not_answer_is_traced() {
        assert!(worth_tracing("connect_failed"));
        assert!(worth_tracing("icmp_packet_loss"));
        assert!(!worth_tracing("dns_failed"));
        assert!(!worth_tracing("assertion_failed"));
        assert!(!worth_tracing("http2_refused"));
    }

    /// The loopback answers at the first hop. An Agent that may not send the probes says so
    /// instead of tracing.
    #[tokio::test]
    async fn traces_the_route_or_says_why_it_cannot() {
        let target = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let evidence = trace(target).await;

        assert_eq!(evidence.action, "traceroute");
        assert_eq!(evidence.metadata.get("target"), Some(&target.to_string()));

        if evidence.error_category.is_empty() {
            let hops: serde_json::Value =
                serde_json::from_str(&evidence.metadata["hops"]).expect("hops are JSON");

            assert_eq!(
                evidence.metadata.get("reached").map(String::as_str),
                Some("true")
            );
            assert_eq!(hops[0][0], 1);
            assert_eq!(hops[0][1], "127.0.0.1");
        } else {
            assert_eq!(evidence.error_category, "traceroute_unavailable");
            assert!(
                evidence.error_message.contains("CAP_NET_RAW"),
                "{}",
                evidence.error_message
            );
        }
    }
}
