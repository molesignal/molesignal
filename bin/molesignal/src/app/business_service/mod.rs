// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Business services: the catalog of what customers rely on, the health that
//! follows from their dependencies, and where that health goes. Every change
//! of health reaches the components that stand for the service: one that
//! follows it takes the status on, and one in automation mode hears of it
//! through the automation rules of its page.

mod catalog;
mod evaluation;
mod signals;

use std::sync::Arc;

pub use catalog::BusinessServiceInput;
pub use signals::{
    BusinessServiceAlertSink, BusinessServiceSignalQueue, BusinessServiceSyntheticSink,
};

use crate::{
    app::status_page::StatusPageService,
    domain::{
        alerting::{
            mute::MuteRuleRepository,
            repositories::{AlertRuleRepository, IncidentRepository},
        },
        business_service::BusinessServiceRepository,
        synthetics::SyntheticRepository,
    },
};

/// What business services read and act on.
pub struct BusinessServiceDependencies {
    pub repository: Arc<dyn BusinessServiceRepository>,
    pub incidents: Arc<dyn IncidentRepository>,
    pub alert_rules: Arc<dyn AlertRuleRepository>,
    pub mute_rules: Arc<dyn MuteRuleRepository>,
    pub synthetics: Arc<dyn SyntheticRepository>,
    pub status_pages: Arc<StatusPageService>,
    pub signals: Arc<BusinessServiceSignalQueue>,
}

pub struct BusinessServiceService {
    repository: Arc<dyn BusinessServiceRepository>,
    incidents: Arc<dyn IncidentRepository>,
    alert_rules: Arc<dyn AlertRuleRepository>,
    mute_rules: Arc<dyn MuteRuleRepository>,
    synthetics: Arc<dyn SyntheticRepository>,
    status_pages: Arc<StatusPageService>,
    signals: Arc<BusinessServiceSignalQueue>,
}

impl BusinessServiceService {
    pub fn new(dependencies: BusinessServiceDependencies) -> Self {
        Self {
            repository: dependencies.repository,
            incidents: dependencies.incidents,
            alert_rules: dependencies.alert_rules,
            mute_rules: dependencies.mute_rules,
            synthetics: dependencies.synthetics,
            status_pages: dependencies.status_pages,
            signals: dependencies.signals,
        }
    }

    pub fn signals(&self) -> &Arc<BusinessServiceSignalQueue> {
        &self.signals
    }
}
