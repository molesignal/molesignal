// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Whether an attempt that went wrong failed, or could not be judged.
//!
//! A target that cannot be resolved or reached, that drops the connection, fails the TLS
//! handshake or does not answer in time has failed the check: that is the outage a check is
//! there to catch. Any other error leaves the attempt unknown, because the check itself could
//! not run: a setting it cannot run with, a policy that holds it back, a secret it lacks.

use std::fmt;

/// The target let the check down.
#[derive(Debug)]
pub(super) struct TargetFailure {
    pub category: &'static str,
    pub message: String,
}

impl fmt::Display for TargetFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TargetFailure {}

/// An error that says the target failed the check, in `category`.
pub(super) fn target(category: &'static str, message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(TargetFailure {
        category,
        message: message.into(),
    })
}

/// The target failure somewhere in `error`, if the target is what went wrong.
pub(super) fn target_failure(error: &anyhow::Error) -> Option<&TargetFailure> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<TargetFailure>())
}

/// `error` said again after `prefix`, still a target failure when it was one.
pub(super) fn restate(error: anyhow::Error, prefix: impl fmt::Display) -> anyhow::Error {
    let message = format!("{prefix}: {error:#}");

    match target_failure(&error) {
        Some(failure) => target(failure.category, message),
        None => anyhow::anyhow!(message),
    }
}

#[cfg(test)]
mod tests {
    use super::{restate, target, target_failure};

    #[test]
    fn a_target_failure_survives_context_and_restating() {
        let failure = target("connect_failed", "connection refused");
        let wrapped = failure.context("while checking");

        assert_eq!(
            target_failure(&wrapped).map(|failure| failure.category),
            Some("connect_failed")
        );

        let restated = restate(wrapped, "error sending request");

        assert_eq!(
            restated.to_string(),
            "error sending request: while checking: connection refused"
        );
        assert!(target_failure(&restated).is_some());
    }

    #[test]
    fn any_other_error_is_not_the_target_failing() {
        let error = anyhow::anyhow!("missing resolved Secret `token`");

        assert!(target_failure(&error).is_none());
        assert!(target_failure(&restate(error, "HTTP step")).is_none());
    }
}
