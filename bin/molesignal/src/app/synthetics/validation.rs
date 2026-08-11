// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::str::FromStr;

use crate::{
    domain::synthetics::{
        BrowserAction, MonitorRevision, MonitorSchedule, MonitorSpec, MultiLocationPolicy,
        ValueSource,
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
            for step in &spec.steps {
                match &step.action {
                    BrowserAction::Navigate { url, .. } => validate_value(url)?,
                    BrowserAction::Fill { value, .. } | BrowserAction::Select { value, .. } => {
                        validate_value(value)?
                    }
                    BrowserAction::WaitDuration { duration_millis }
                        if *duration_millis > 30_000 =>
                    {
                        return Err(Error::invalid("Browser wait cannot exceed 30 seconds"));
                    }
                    _ => {}
                }
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
