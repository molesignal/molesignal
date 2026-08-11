// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

mod config;
mod engine;
mod events;
mod matcher;
mod quiet;
mod registry;
mod resolvers;
mod service;

pub use config::mask_connector_config;
pub use engine::{
    NotifyDefaultInput, NotifyDeliveryPlanStep, NotifyDispatch, NotifyEngine,
    NotifyEngineDependencies, NotifyEventOutcome, NotifyPolicyInput, NotifyPolicyOutcome,
    NotifyPolicyPreview, NotifyRecipientOutcome, NotifyRecipientPlan,
    validate_notify_template_body,
};
pub use events::{
    ALERT_ACKNOWLEDGED_EVENT, ALERT_ESCALATED_EVENT, ALERT_RESOLVED_EVENT, ALERT_TRIGGERED_EVENT,
    ONCALL_COVERAGE_MISSING_EVENT, ONCALL_OVERRIDE_CREATED_EVENT, ONCALL_SHIFT_STARTED_EVENT,
    ONCALL_SHIFT_STARTING_EVENT, OncallEventProducer, alert_dispatch, alert_escalation_dispatch,
    override_created_dispatch, triggered_event_id,
};
pub use matcher::{policy_matches, validate_matchers};
pub use quiet::{quiet_hours_active, validate_quiet_hours};
pub use registry::ConnectorRegistry;
pub use resolvers::{
    ALERT_OWNER_RESOLVER, AlertOwnerResolver, CURRENT_ONCALL_RESOLVER, CurrentOncallResolver,
    EVENT_USERS_RESOLVER, EventUsersResolver, FIXED_USERS_RESOLVER, FixedUsersResolver,
    NEXT_ONCALL_RESOLVER, NextOncallResolver, RecipientResolverRegistry, SCHEDULE_MEMBERS_RESOLVER,
    ScheduleMembersResolver, TEAM_LEAD_RESOLVER, TEAM_MEMBERS_RESOLVER, TeamLeadResolver,
    TeamMembersResolver,
};
pub use service::{
    ConnectorTestOutcome, CreateNotifyConnector, CreateUserNotifyEndpoint, NotifyService,
    UpdateNotifyConnector, UpdateUserNotifyEndpoint,
};
