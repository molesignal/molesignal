// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a browser check has to do with the RUM session of the page it drives: it tells the page that
//! a test drives it, and at the end of the journey reads which session the SDK recorded.

use std::time::Duration;

use super::{BrowserRuntime, actions};
use crate::protocol::v1 as wire;

/// How long each command to read the session and to leave the page may take.
const RUM_COMMAND_TIMEOUT: Duration = Duration::from_millis(500);
/// How long a page that sends RUM data gets to deliver its last events once it has been left.
const RUM_DELIVERY_GRACE: Duration = Duration::from_millis(800);
/// Where a page finds that a test drives it, and where the SDK says the session it then records.
const TEST_CONTEXT_GLOBAL: &str = "__MOLESIGNAL_TEST_CONTEXT__";
const RUM_SESSION_GLOBAL: &str = "__MOLESIGNAL_RUM_SESSION__";

/// The script that tells every document of the journey that a check drives it. A page that runs the
/// RUM SDK then records its session as a synthetic one, under this check and this run.
pub(super) fn test_context_script(task: &wire::ProbeTask) -> String {
    let context = serde_json::json!({
        "type": "synthetics",
        "test_id": task.monitor_id,
        "result_id": task.task_id,
        "location_id": task.location_id,
    });

    format!("window.{TEST_CONTEXT_GLOBAL} = Object.freeze({context});")
}

impl BrowserRuntime {
    /// Leaves a page whose RUM SDK says it records a session, so that the SDK sends what it holds
    /// as a page does when a person leaves, and gives that a moment to arrive. Says the session; a
    /// page that records none is left as it is. A journey that ran out of time does this too, as
    /// the one a reader most wants to open the session of: these bounds fit `FINALIZATION_GRACE`.
    pub(super) fn leave_recorded_page(&mut self) -> Option<String> {
        self.tab.set_default_timeout(RUM_COMMAND_TIMEOUT);

        let session = actions::evaluate_string(
            &self.tab,
            &format!("String(window.{RUM_SESSION_GLOBAL} || '')"),
        )
        .ok()
        .filter(|session| !session.is_empty())?;

        if self.tab.navigate_to("about:blank").is_ok() {
            std::thread::sleep(RUM_DELIVERY_GRACE);
        }

        Some(session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_told_which_check_and_which_run_drive_it() {
        let script = test_context_script(&wire::ProbeTask {
            task_id: "task-1".into(),
            monitor_id: "check-\"1".into(),
            location_id: "eu-west".into(),
            ..wire::ProbeTask::default()
        });

        assert!(
            script.starts_with("window.__MOLESIGNAL_TEST_CONTEXT__ = Object.freeze({"),
            "{script}"
        );

        let context = script
            .trim_start_matches("window.__MOLESIGNAL_TEST_CONTEXT__ = Object.freeze(")
            .trim_end_matches(");");

        let context: serde_json::Value = serde_json::from_str(context).unwrap();

        assert_eq!(context["type"], "synthetics");
        assert_eq!(context["test_id"], "check-\"1");
        assert_eq!(context["result_id"], "task-1");
        assert_eq!(context["location_id"], "eu-west");
    }
}
