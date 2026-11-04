// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! An alert pipeline's HTTP enrich step: it calls one of the organization's HTTP connectors, whose
//! address, credential and outbound checks are the connector's.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value};

use super::{ConnectorKind, ConnectorRepository, dispatcher::http};
use crate::{
    domain::pipeline::alert::AlertEnricher,
    shared::{Error, Result, ids::Id},
};

pub struct ConnectorEnricher {
    connectors: Arc<dyn ConnectorRepository>,
}

impl ConnectorEnricher {
    pub fn new(connectors: Arc<dyn ConnectorRepository>) -> Self {
        Self { connectors }
    }
}

#[async_trait]
impl AlertEnricher for ConnectorEnricher {
    async fn call(
        &self,
        org: &Id,
        connector: &Id,
        body: &Value,
    ) -> Result<Option<Map<String, Value>>> {
        let connector = self.connectors.get(org, connector).await?;

        if connector.kind != ConnectorKind::Http.as_str() {
            return Err(Error::invalid(format!(
                "connector `{}` is not an HTTP connector",
                connector.name
            )));
        }

        if !connector.enabled {
            return Err(Error::invalid(format!(
                "connector `{}` is switched off",
                connector.name
            )));
        }

        http::enrich(&connector, body).await
    }
}
