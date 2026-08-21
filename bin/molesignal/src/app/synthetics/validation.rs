// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::str::FromStr;

use crate::{
    domain::synthetics::{
        BrowserAction, MonitorRevision, MonitorSchedule, MonitorSpec, MultiLocationPolicy,
        SshAuthentication, ValueSource,
    },
    shared::{Error, Result},
};

pub fn validate_revision(revision: &MonitorRevision) -> Result<()> {
    if revision.timeout_millis < 100 || revision.timeout_millis > 300_000 {
        return Err(Error::invalid(
            "Monitor timeout must be between 100 ms and 5 minutes",
        ));
    }
    if revision.max_retries > 2 {
        return Err(Error::invalid("Monitor retries cannot exceed two"));
    }
    if revision.consecutive_failures == 0 || revision.consecutive_recoveries == 0 {
        return Err(Error::invalid(
            "Monitor transition thresholds must be positive",
        ));
    }
    validate_schedule(&revision.schedule, &revision.spec, revision.timeout_millis)?;
    match &revision.location_policy {
        MultiLocationPolicy::Quorum { required }
            if *required == 0 || *required as usize > revision.location_ids.len() =>
        {
            return Err(Error::invalid("Location quorum exceeds selected Locations"));
        }
        _ => {}
    }
    match &revision.spec {
        MonitorSpec::Http(spec) => {
            if spec.steps.is_empty() || spec.steps.len() > 50 {
                return Err(Error::invalid("HTTP journeys require 1 to 50 steps"));
            }
            for step in &spec.steps {
                if !matches!(
                    step.method.as_str(),
                    "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS"
                ) {
                    return Err(Error::invalid("unsupported HTTP method"));
                }
                validate_value(&step.url)?;
                if let Some(body) = &step.body {
                    validate_value(body)?;
                }
            }
        }
        MonitorSpec::Browser(spec) => {
            if spec.steps.is_empty() || spec.steps.len() > 100 {
                return Err(Error::invalid("Browser journeys require 1 to 100 steps"));
            }
            if spec.viewport.width == 0
                || spec.viewport.width > 7680
                || spec.viewport.height == 0
                || spec.viewport.height > 4320
            {
                return Err(Error::invalid(
                    "Browser viewport is outside the supported range",
                ));
            }
            if spec
                .user_agent
                .as_ref()
                .is_some_and(|value| value.len() > 1024)
            {
                return Err(Error::invalid("Browser user agent is too long"));
            }
            let screenshot_count = spec
                .steps
                .iter()
                .filter(|step| matches!(&step.action, BrowserAction::Screenshot { .. }))
                .count();
            let failure_artifact_count = usize::from(spec.capture_screenshot_on_failure)
                + usize::from(spec.capture_har_on_failure)
                + usize::from(spec.capture_trace_on_failure);
            if screenshot_count + failure_artifact_count > 10 {
                return Err(Error::invalid(
                    "Browser journeys cannot configure more than 10 Artifacts per Attempt",
                ));
            }
            for step in &spec.steps {
                match &step.action {
                    BrowserAction::Navigate { url, wait_until } => {
                        validate_value(url)?;
                        if !matches!(
                            wait_until.to_ascii_lowercase().as_str(),
                            "" | "load"
                                | "domcontentloaded"
                                | "networkidle"
                                | "network_idle"
                                | "commit"
                                | "none"
                        ) {
                            return Err(Error::invalid(
                                "unsupported Browser navigation wait condition",
                            ));
                        }
                    }
                    BrowserAction::Click { selector }
                    | BrowserAction::WaitSelector { selector }
                        if selector.trim().is_empty() =>
                    {
                        return Err(Error::invalid("Browser selector cannot be empty"));
                    }
                    BrowserAction::Fill { selector, value }
                    | BrowserAction::Select { selector, value } => {
                        if selector.trim().is_empty() {
                            return Err(Error::invalid("Browser selector cannot be empty"));
                        }
                        validate_value(value)?;
                    }
                    BrowserAction::WaitDuration { duration_millis }
                        if *duration_millis > 30_000 =>
                    {
                        return Err(Error::invalid("Browser wait cannot exceed 30 seconds"));
                    }
                    BrowserAction::WaitExpression { expression }
                        if expression.trim().is_empty() || expression.len() > 64 * 1024 =>
                    {
                        return Err(Error::invalid("invalid Browser wait expression"));
                    }
                    BrowserAction::Extract {
                        variable,
                        selector,
                        source,
                    } if variable.trim().is_empty()
                        || variable.len() > 128
                        || selector.trim().is_empty()
                        || source.trim().is_empty() =>
                    {
                        return Err(Error::invalid("invalid Browser extraction"));
                    }
                    BrowserAction::Screenshot { name, .. }
                        if name.trim().is_empty() || name.len() > 128 =>
                    {
                        return Err(Error::invalid("invalid Browser screenshot name"));
                    }
                    _ => {}
                }
            }
        }
        MonitorSpec::Ssh(spec) => {
            validate_value(&spec.host)?;
            if spec.port == 0 {
                return Err(Error::invalid("SSH port must be between 1 and 65535"));
            }
            if let Some(pattern) = &spec.expected_identification_regex {
                regex::Regex::new(pattern).map_err(|error| {
                    Error::invalid(format!("invalid SSH identification regex: {error}"))
                })?;
            }
            if let Some(pattern) = &spec.expected_output_regex {
                regex::Regex::new(pattern).map_err(|error| {
                    Error::invalid(format!("invalid SSH output regex: {error}"))
                })?;
            }
            if spec.authentication.is_some()
                && spec
                    .expected_host_key_sha256
                    .as_ref()
                    .is_none_or(|fingerprint| {
                        !regex::Regex::new(r"^SHA256:[A-Za-z0-9+/]{43}$")
                            .expect("static SSH fingerprint regex")
                            .is_match(fingerprint)
                    })
            {
                return Err(Error::invalid(
                    "SSH authentication requires a SHA256 host key fingerprint",
                ));
            }
            if let Some(authentication) = &spec.authentication {
                match authentication {
                    SshAuthentication::Password { username, password } => {
                        validate_nonempty_value(username, "SSH username")?;
                        validate_nonempty_value(password, "SSH password")?;
                    }
                    SshAuthentication::PublicKey {
                        username,
                        private_key,
                        passphrase,
                    } => {
                        validate_nonempty_value(username, "SSH username")?;
                        validate_nonempty_value(private_key, "SSH private key")?;
                        if let Some(passphrase) = passphrase {
                            validate_nonempty_value(passphrase, "SSH private key passphrase")?;
                        }
                    }
                }
            }
            if let Some(command) = &spec.command {
                if spec.authentication.is_none() {
                    return Err(Error::invalid("SSH command requires authentication"));
                }
                validate_nonempty_value(command, "SSH command")?;
            } else if spec.expected_output_regex.is_some() || spec.expected_exit_status.is_some() {
                return Err(Error::invalid(
                    "SSH output and exit status expectations require a command",
                ));
            }
        }
        MonitorSpec::Heartbeat if !revision.location_ids.is_empty() => {
            return Err(Error::invalid("Heartbeat Monitors are Location-free"));
        }
        _ => {}
    }
    Ok(())
}

fn validate_schedule(
    schedule: &MonitorSchedule,
    spec: &MonitorSpec,
    timeout_millis: u32,
) -> Result<()> {
    match schedule {
        MonitorSchedule::Interval { every_seconds } => {
            let minimum = if matches!(spec, MonitorSpec::Browser(_)) {
                60
            } else {
                10
            };
            if *every_seconds < minimum || *every_seconds > 86_400 {
                return Err(Error::invalid(format!(
                    "Monitor interval must be between {minimum} seconds and one day"
                )));
            }
            if u64::from(timeout_millis) >= u64::from(*every_seconds) * 1000 {
                return Err(Error::invalid(
                    "Monitor timeout must be shorter than its interval",
                ));
            }
        }
        MonitorSchedule::Cron {
            expression,
            timezone,
        } => {
            cron::Schedule::from_str(expression)
                .map_err(|error| Error::invalid(format!("invalid Cron expression: {error}")))?;
            chrono_tz::Tz::from_str(timezone)
                .map_err(|_| Error::invalid("invalid IANA timezone"))?;
        }
        MonitorSchedule::Heartbeat {
            expected_seconds,
            grace_seconds,
        } => {
            if !matches!(spec, MonitorSpec::Heartbeat) {
                return Err(Error::invalid(
                    "Heartbeat schedule requires a Heartbeat Monitor",
                ));
            }
            if *expected_seconds < 10 || *grace_seconds > expected_seconds.saturating_mul(10) {
                return Err(Error::invalid("invalid Heartbeat interval or grace window"));
            }
        }
    }
    Ok(())
}

fn validate_value(value: &ValueSource) -> Result<()> {
    match value {
        ValueSource::Literal { value } if value.len() > 64 * 1024 => {
            Err(Error::invalid("Monitor literal exceeds 64 KiB"))
        }
        ValueSource::Variable { name }
        | ValueSource::Secret {
            reference: name, ..
        } if name.is_empty() || name.len() > 128 => Err(Error::invalid(
            "invalid Monitor variable or Secret reference",
        )),
        _ => Ok(()),
    }
}

fn validate_nonempty_value(value: &ValueSource, label: &str) -> Result<()> {
    validate_value(value)?;
    if matches!(value, ValueSource::Literal { value } if value.trim().is_empty()) {
        return Err(Error::invalid(format!("{label} cannot be empty")));
    }
    Ok(())
}
