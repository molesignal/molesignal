// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 告警用例：规则 CRUD、Incident 处理（确认、解决、转派、升级、暂缓、合并、备注）、排班查询、
//! 升级派发编排，以及接入外部系统的告警。

pub mod actions;
pub mod apm_source;
pub mod discovery;
pub mod dispatcher;
pub mod event_pipelines;
pub mod followups;
pub mod holidays;
pub mod importance;
pub mod inhibitions;
pub mod insights;
mod lifecycle;
pub mod oncall;
pub mod oncall_transfer;
pub mod postmortems;
pub mod realtime;
pub mod related;
pub mod remediation;
pub mod response;
pub mod routing;
pub mod rule_evaluator;
pub mod rule_transfer;
pub mod rum_source;
pub mod service;
pub mod sources;
pub mod tickets;
pub mod tokens;

pub use actions::{Actor, IncidentActions, IncidentMerger, MergedRecoverySink};
pub use apm_source::{ApmAlertReading, ApmAlertSlot, ApmAlertSource};
pub use dispatcher::EscalationDispatcher;
pub use holidays::HolidayCalendars;
pub use importance::ImportanceService;
pub use inhibitions::Inhibitions;
pub use insights::AlertInsights;
pub use lifecycle::{AlertIncidentLifecycleSink, AlertIncidentLifecycleSinks};
pub use oncall::{CalendarFeeds, ShiftSwaps};
pub use postmortems::PostmortemService;
pub use related::RelatedIncidents;
pub use remediation::RemediationService;
pub use response::{IncidentResponseService, ResponseLifecycleSink};
pub use routing::{AlertRouting, IncidentRouter};
pub use rule_evaluator::{RuleEvaluator, SyntheticIncidentSink};
pub use service::AlertingService;
pub use sources::AlertSources;
pub use tickets::{TicketLifecycleSink, TicketService};
