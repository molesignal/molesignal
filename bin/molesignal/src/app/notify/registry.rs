// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashMap, sync::Arc};

use crate::{
    domain::notify::connector::{ConnectorAdapter, ConnectorCapabilities},
    shared::{Error, Result},
};

/// 启动期构建、运行期只读的连接器 adapter 注册表。
pub struct ConnectorRegistry {
    adapters: HashMap<&'static str, Arc<dyn ConnectorAdapter>>,
}

impl ConnectorRegistry {
    pub fn new(adapters: impl IntoIterator<Item = Arc<dyn ConnectorAdapter>>) -> Result<Self> {
        let mut registry = Self {
            adapters: HashMap::new(),
        };
        for adapter in adapters {
            let connector_type = adapter.connector_type();
            if connector_type.trim().is_empty() {
                return Err(Error::internal("connector adapter type cannot be empty"));
            }
            if registry.adapters.insert(connector_type, adapter).is_some() {
                return Err(Error::internal(format!(
                    "duplicate connector adapter: {connector_type}"
                )));
            }
        }
        Ok(registry)
    }

    pub fn get(&self, connector_type: &str) -> Result<Arc<dyn ConnectorAdapter>> {
        self.adapters
            .get(connector_type)
            .cloned()
            .ok_or_else(|| Error::invalid(format!("unsupported connector type: {connector_type}")))
    }

    pub fn capabilities(&self, connector_type: &str) -> Result<ConnectorCapabilities> {
        Ok(self.get(connector_type)?.capabilities())
    }

    pub fn supported_types(&self) -> Vec<(&'static str, ConnectorCapabilities)> {
        let mut supported = self
            .adapters
            .iter()
            .map(|(connector_type, adapter)| (*connector_type, adapter.capabilities()))
            .collect::<Vec<_>>();
        supported.sort_by_key(|(connector_type, _)| *connector_type);
        supported
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use serde_json::Value;

    use super::*;
    use crate::domain::notify::connector::{ConnectorDeliveryResult, NotifyMessage, NotifyTarget};

    struct FakeAdapter(&'static str);

    #[async_trait]
    impl ConnectorAdapter for FakeAdapter {
        fn connector_type(&self) -> &'static str {
            self.0
        }

        fn capabilities(&self) -> ConnectorCapabilities {
            ConnectorCapabilities {
                direct_user: true,
                ..ConnectorCapabilities::default()
            }
        }

        fn validate_config(&self, _config: &Value) -> Result<()> {
            Ok(())
        }

        fn validate_target(&self, _target: &NotifyTarget) -> Result<()> {
            Ok(())
        }

        async fn send(
            &self,
            _config: &Value,
            _target: &NotifyTarget,
            _message: &NotifyMessage,
        ) -> Result<ConnectorDeliveryResult> {
            unreachable!("registry lookup test does not send")
        }
    }

    #[test]
    fn registry_resolves_by_type_without_brand_branching() {
        let registry =
            ConnectorRegistry::new([Arc::new(FakeAdapter("email_smtp")) as Arc<_>]).unwrap();
        assert!(registry.capabilities("email_smtp").unwrap().direct_user);
        assert!(registry.get("unknown").is_err());
    }

    #[test]
    fn registry_rejects_duplicate_types() {
        let result = ConnectorRegistry::new([
            Arc::new(FakeAdapter("email_smtp")) as Arc<_>,
            Arc::new(FakeAdapter("email_smtp")) as Arc<_>,
        ]);
        assert!(result.is_err());
    }
}
