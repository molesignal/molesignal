// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The APM settings of each organization as queries read them: read again
//! once [`FRESH_FOR`] has passed, and at once after a save on this node.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::RwLock;

use crate::{
    domain::apm::{ApmCatalogRepository, ApmOrgSettings},
    shared::ids::Id,
};

/// How long settings saved on another node take at most to be read here.
const FRESH_FOR: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(super) struct OrgSettingsCache {
    /// Where settings are kept; without it every organization has the defaults.
    catalog: Option<Arc<dyn ApmCatalogRepository>>,
    read: RwLock<HashMap<Id, (Instant, Arc<ApmOrgSettings>)>>,
}

impl OrgSettingsCache {
    pub(super) fn new(catalog: Arc<dyn ApmCatalogRepository>) -> Self {
        Self {
            catalog: Some(catalog),
            read: RwLock::default(),
        }
    }

    /// The organization's settings; when they cannot be read, the ones read
    /// before, else the defaults, so that a page never fails for them.
    pub(super) async fn get(&self, org_id: &Id) -> Arc<ApmOrgSettings> {
        let kept = self.read.read().get(org_id).cloned();

        if let Some((read_at, settings)) = &kept
            && read_at.elapsed() < FRESH_FOR
        {
            return settings.clone();
        }

        let Some(catalog) = &self.catalog else {
            return Arc::default();
        };

        match catalog.get_settings(org_id).await {
            Ok(settings) => {
                let settings = Arc::new(settings.unwrap_or_default());

                self.read
                    .write()
                    .insert(org_id.clone(), (Instant::now(), settings.clone()));

                settings
            }
            Err(error) => {
                tracing::warn!(
                    target: "molesignal::app::apm",
                    org_id = %org_id,
                    error = %error,
                    "APM settings could not be read; reading with the settings read before"
                );

                kept.map(|(_, settings)| settings).unwrap_or_default()
            }
        }
    }

    pub(super) fn forget(&self, org_id: &Id) {
        self.read.write().remove(org_id);
    }
}
