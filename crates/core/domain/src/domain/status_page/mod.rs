// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Customer-facing status pages and their public incident timelines.

mod access;
mod automation;
mod component_group;
mod custom_domain;
mod model;
mod postmortem;
mod query;
mod repository;
mod service_link;
mod subscriber_import;
mod subscription;

pub use access::*;
pub use automation::*;
pub use component_group::{ComponentKind, group_members, settle_component_groups};
pub use custom_domain::*;
pub use model::{
    ComponentLifecycle, ComponentStatus, ComponentVisibility, IncidentImpact, PublicIncidentStatus,
    StatusPage, StatusPageComponent, StatusPageComponentStatusEvent, StatusPageIncident,
    StatusPageIncidentKind, StatusPageIncidentUpdate, StatusPageLifecycle,
    StatusPagePublicationState, StatusPageSnapshot, StatusPageVisibility,
};
pub use postmortem::{PostmortemWrite, StatusPagePostmortem, StatusPagePostmortemRepository};
pub use query::*;
pub use repository::{
    MaintenanceDueToStart, StatusPageComponentRepository, StatusPageConfigRepository,
    StatusPageDomainRepository, StatusPageIncidentQueryRepository, StatusPageIncidentRepository,
    StatusPageIncidentWriteRepository, StatusPagePageRepository,
    StatusPagePlannedMaintenanceRepository, StatusPageRepository,
};
pub use service_link::{ComponentServiceLink, ServiceLinkMode, StatusPageServiceLinkRepository};
pub use subscriber_import::{
    ImportedStatusPageSubscriber, StatusPageSubscriberImport, StatusPageSubscriberImportMode,
    StatusPageSubscriberImportOutcome,
};
pub use subscription::{
    PendingStatusPageSubscription, SlackAppCompletion, SlackAppConnectorChoice,
    SlackIncomingWebhook, StatusPageDeliveryPage, StatusPageDeliveryRepository,
    StatusPageDeliveryStatus, StatusPageNotificationDelivery, StatusPageNotificationMessage,
    StatusPageNotificationSender, StatusPageSlackApp, StatusPageSlackAppRepository,
    StatusPageSlackApps, StatusPageSubscriber, StatusPageSubscriberChannel,
    StatusPageSubscriberOrigin, StatusPageSubscriberPage, StatusPageSubscriberRepository,
    StatusPageSubscriberStatus, StatusPageSubscriptionRepository, SubscriberComponentScope,
    SubscriberComponents, SubscriptionRequestAction,
};
