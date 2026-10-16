// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Business services: what customers rely on, the technical services,
//! monitors and alert rules they depend on, and the health that follows from
//! those. The health feeds the internal view and, through the components that
//! stand for a service, the public status pages.

mod health;
mod model;
mod repository;

pub use health::{Evaluation, HealthSignals, IncidentSignal, MonitorSignal, evaluate};
pub use model::*;
pub use repository::{BusinessServiceRepository, HealthWrite};
