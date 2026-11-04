// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod alert;
mod intake_budget;
mod oncall;

pub use alert::{
    ALERT_ACKNOWLEDGED_EVENT, ALERT_ESCALATED_EVENT, ALERT_RESOLVED_EVENT, ALERT_TRIGGERED_EVENT,
    alert_dispatch, alert_escalation_dispatch, alert_page_dispatch, escalated_event_prefix,
    retold_by_pipeline, triggered_event_id,
};
#[cfg(test)]
pub(super) use intake_budget::budget_dispatch;
pub use intake_budget::{
    INTAKE_BUDGET_EXCEEDED_EVENT, INTAKE_BUDGET_WARNING_EVENT, IntakeBudgetEventProducer,
};
pub use oncall::{
    ONCALL_COVERAGE_MISSING_EVENT, ONCALL_MEMBER_UNREACHABLE_EVENT, ONCALL_OVERRIDE_CREATED_EVENT,
    ONCALL_SHIFT_STARTED_EVENT, ONCALL_SHIFT_STARTING_EVENT, ONCALL_SWAP_UPDATED_EVENT,
    OncallEventProducer, override_created_dispatch, swap_dispatch,
};
