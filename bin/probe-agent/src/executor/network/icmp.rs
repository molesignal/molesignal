// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! ICMP echo: how many of a few pings come back, and how fast.

use std::{
    collections::HashMap,
    io,
    net::IpAddr,
    time::{Duration, Instant},
};

use anyhow::Result;
use surge_ping::{Client, Config, ICMP, PingIdentifier, PingSequence, SurgeError};

use super::super::{
    AttemptOutcome, ExecutionContext, security::EgressGuard, timing::PhaseClock, unknown,
};
use crate::protocol::v1::{self as wire, IcmpSpec, ProbeOutcome};

/// A ping carries the 56 bytes the `ping` command sends.
const PAYLOAD: [u8; 56] = [0; 56];

/// The longest one ping waits for its reply.
const PING_WAIT: Duration = Duration::from_secs(2);

const MAX_COUNT: u32 = 20;

/// Whether this process may send pings. The socket is opened unprivileged where the system
/// allows it (macOS, and Linux within `net.ipv4.ping_group_range`), else as a raw socket, which
/// needs `CAP_NET_RAW`. An Agent that can do neither does not offer ICMP checks.
pub fn available() -> bool {
    Client::new(&Config::default()).is_ok()
}

pub(in super::super) async fn execute(
    task: &wire::ProbeTask,
    spec: &IcmpSpec,
    context: &ExecutionContext,
    phases: &PhaseClock,
) -> Result<AttemptOutcome> {
    let guard = EgressGuard::new(task.egress_policy.clone())?;
    let address = guard.resolve_ips(&spec.host, phases).await?.remove(0);
    context.dialling(address);

    let config = match address {
        IpAddr::V4(_) => Config::default(),
        IpAddr::V6(_) => Config::builder().kind(ICMP::V6).build(),
    };

    let client = match Client::new(&config) {
        Ok(client) => client,
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            return Ok(unknown(
                "icmp_permission_denied",
                "this Agent may not send pings: give it CAP_NET_RAW, or allow its group in net.ipv4.ping_group_range",
            ));
        }
        Err(error) => return Err(error.into()),
    };

    let mut pinger = client.pinger(address, PingIdentifier(rand::random())).await;
    let count = spec.count.clamp(1, MAX_COUNT);
    let interval = Duration::from_millis(u64::from(spec.interval_millis));
    let deadline = Instant::now() + Duration::from_millis(u64::from(task.timeout_millis.max(1)));
    let mut replies = Vec::new();

    for sequence in 0..count {
        let left = deadline.saturating_duration_since(Instant::now());

        // A ping the attempt has no time left for is one that did not come back.
        if left.is_zero() {
            break;
        }

        pinger.timeout(left.min(PING_WAIT));

        match pinger.ping(PingSequence(sequence as u16), &PAYLOAD).await {
            Ok((_, round_trip)) => replies.push(round_trip),
            Err(SurgeError::Timeout { .. }) => {}
            Err(error) => return Err(error.into()),
        }

        if sequence + 1 < count {
            tokio::time::sleep(interval.min(deadline.saturating_duration_since(Instant::now())))
                .await;
        }
    }

    Ok(judge(spec, count, &replies, address))
}

fn judge(spec: &IcmpSpec, sent: u32, replies: &[Duration], address: IpAddr) -> AttemptOutcome {
    let received = u32::try_from(replies.len()).unwrap_or(u32::MAX);
    let loss = 1.0 - f64::from(received) / f64::from(sent.max(1));
    let millis = |duration: &Duration| duration.as_secs_f64() * 1000.0;
    let mean =
        (received > 0).then(|| replies.iter().map(millis).sum::<f64>() / f64::from(received));

    let mut metadata = HashMap::from([
        ("remote_address".into(), address.to_string()),
        ("sent".into(), sent.to_string()),
        ("received".into(), received.to_string()),
        ("packet_loss".into(), format!("{loss:.3}")),
    ]);

    if let Some(mean) = mean {
        let fastest = replies.iter().map(millis).fold(f64::INFINITY, f64::min);
        let slowest = replies.iter().map(millis).fold(0.0, f64::max);

        metadata.insert("rtt_min_ms".into(), format!("{fastest:.2}"));
        metadata.insert("rtt_mean_ms".into(), format!("{mean:.2}"));
        metadata.insert("rtt_max_ms".into(), format!("{slowest:.2}"));
    }

    // Nothing back at all is a host out of reach; some back is a lossy or a slow path.
    let (outcome, category, message) = if received == 0 {
        (
            ProbeOutcome::Failing,
            "icmp_no_reply",
            format!("no reply from {address} to {sent} pings"),
        )
    } else if loss > spec.max_packet_loss_ratio {
        (
            ProbeOutcome::Failing,
            "icmp_packet_loss",
            format!("{:.0}% of pings to {address} were lost", loss * 100.0),
        )
    } else if let (Some(limit), Some(mean)) = (spec.max_mean_rtt_millis, mean)
        && mean > f64::from(limit)
    {
        (
            ProbeOutcome::Degraded,
            "icmp_slow",
            format!("pings to {address} took {mean:.1} ms on average, above {limit} ms"),
        )
    } else {
        (ProbeOutcome::Healthy, "", String::new())
    };

    AttemptOutcome {
        outcome,
        assertions: Vec::new(),
        response_excerpt: Vec::new(),
        error_category: category.into(),
        error_message: message,
        metadata,
        evidence: Vec::new(),
        artifacts: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::{net::Ipv4Addr, time::Duration};

    use super::judge;
    use crate::protocol::v1::{IcmpSpec, ProbeOutcome};

    fn spec(max_loss: f64, max_mean: Option<u32>) -> IcmpSpec {
        IcmpSpec {
            host: "192.0.2.1".into(),
            count: 4,
            interval_millis: 200,
            max_packet_loss_ratio: max_loss,
            max_mean_rtt_millis: max_mean,
        }
    }

    #[test]
    fn no_reply_fails_and_some_loss_is_judged_by_the_limit() {
        let address = Ipv4Addr::LOCALHOST.into();
        let reply = Duration::from_millis(10);

        assert_eq!(
            judge(&spec(0.5, None), 4, &[], address).outcome,
            ProbeOutcome::Failing
        );

        let lossy = judge(&spec(0.2, None), 4, &[reply, reply, reply], address);
        assert_eq!(lossy.outcome, ProbeOutcome::Failing);
        assert_eq!(lossy.metadata["packet_loss"], "0.250");

        let tolerated = judge(&spec(0.25, None), 4, &[reply, reply, reply], address);
        assert_eq!(tolerated.outcome, ProbeOutcome::Healthy);
    }

    #[test]
    fn a_slow_path_degrades() {
        let address = Ipv4Addr::LOCALHOST.into();
        let replies = [Duration::from_millis(30), Duration::from_millis(50)];
        let slow = judge(&spec(0.0, Some(20)), 2, &replies, address);

        assert_eq!(slow.outcome, ProbeOutcome::Degraded);
        assert_eq!(slow.metadata["rtt_mean_ms"], "40.00");
    }
}
