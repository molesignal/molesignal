// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Customer-facing status pages and their public incident timelines.

mod access;
mod custom_domain;
mod model;
mod query;
mod repository;
mod subscription;

pub use access::*;
pub use custom_domain::*;
pub use model::{
    ComponentLifecycle, ComponentStatus, ComponentVisibility, IncidentImpact, PublicIncidentStatus,
    StatusPage, StatusPageComponent, StatusPageComponentStatusEvent, StatusPageIncident,
    StatusPageIncidentKind, StatusPageIncidentUpdate, StatusPageLifecycle,
    StatusPagePublicationState, StatusPageSnapshot, StatusPageVisibility,
};
pub use query::*;
pub use repository::{
    StatusPageComponentRepository, StatusPageConfigRepository, StatusPageDomainRepository,
    StatusPageIncidentQueryRepository, StatusPageIncidentRepository,
    StatusPageIncidentWriteRepository, StatusPagePageRepository, StatusPageRepository,
};
pub use subscription::{
    PendingStatusPageSubscription, StatusPageDeliveryPage, StatusPageDeliveryRepository,
    StatusPageDeliveryStatus, StatusPageNotificationDelivery, StatusPageNotificationMessage,
    StatusPageNotificationSender, StatusPageSubscriber, StatusPageSubscriberChannel,
    StatusPageSubscriberPage, StatusPageSubscriberRepository, StatusPageSubscriberStatus,
    StatusPageSubscriptionRepository,
};
