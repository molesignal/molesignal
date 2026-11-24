// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What APM knows of the services in view, as recorded rather than measured:
//! each service, the versions it reported and the instances it ran as within
//! the range. Reads that ask how well services can be tied to other data
//! start from here.

use domain::apm::{InstanceObservation, ServiceObservation, VersionObservation};
use kernel::Result;

use super::{ApmQueryContext, ApmQueryService};

/// The services in view with their versions and instances.
#[derive(Debug, Clone, Default)]
pub struct ServiceInventory {
    pub services: Vec<ServiceObservation>,
    pub versions: Vec<VersionObservation>,
    pub instances: Vec<InstanceObservation>,
}

impl ApmQueryService {
    /// The services, versions and instances recorded within the context's range, with the
    /// filters it names.
    pub async fn inventory(&self, context: &ApmQueryContext) -> Result<ServiceInventory> {
        let query = self.catalog_query(context);
        let (services, versions, instances) = tokio::try_join!(
            self.repository.list_services(&query),
            self.repository.list_versions(&query),
            self.repository.list_instances(&query),
        )?;

        Ok(ServiceInventory {
            services,
            versions,
            instances,
        })
    }
}
