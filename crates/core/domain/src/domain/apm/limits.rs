// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The most distinct values APM records an hour: services an organization
//! has, endpoints, targets called, operations of a target, error groups,
//! versions and instances a service has. A platform setting, read by every
//! node that reads spans; past a limit the value is counted as `__other__`
//! (a service is not recorded at all).

use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result};

/// The most any limit may be.
pub const APM_LIMIT_MAX: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ApmLimits {
    pub services_per_org_hour: usize,
    pub transactions_per_service_hour: usize,
    pub dependencies_per_service_hour: usize,
    /// Operations of one target a service keeps apart, SQL templates among them.
    pub operations_per_dependency_hour: usize,
    pub error_groups_per_service_hour: usize,
    pub versions_per_service_hour: usize,
    pub instances_per_service_hour: usize,
}

impl Default for ApmLimits {
    fn default() -> Self {
        Self {
            services_per_org_hour: 200,
            transactions_per_service_hour: 32,
            dependencies_per_service_hour: 16,
            operations_per_dependency_hour: 32,
            error_groups_per_service_hour: 16,
            versions_per_service_hour: 16,
            instances_per_service_hour: 256,
        }
    }
}

impl ApmLimits {
    pub fn validate(&self) -> Result<()> {
        let limits = [
            self.services_per_org_hour,
            self.transactions_per_service_hour,
            self.dependencies_per_service_hour,
            self.operations_per_dependency_hour,
            self.error_groups_per_service_hour,
            self.versions_per_service_hour,
            self.instances_per_service_hour,
        ];

        if limits
            .iter()
            .any(|limit| !(1..=APM_LIMIT_MAX).contains(limit))
        {
            return Err(Error::invalid(format!(
                "APM limits are from 1 to {APM_LIMIT_MAX}"
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_limit_is_from_1_to_the_most() {
        assert!(ApmLimits::default().validate().is_ok());
        assert!(
            ApmLimits {
                instances_per_service_hour: 0,
                ..ApmLimits::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ApmLimits {
                services_per_org_hour: APM_LIMIT_MAX + 1,
                ..ApmLimits::default()
            }
            .validate()
            .is_err()
        );
    }
}
