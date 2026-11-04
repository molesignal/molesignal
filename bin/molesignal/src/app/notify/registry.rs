// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashMap, sync::Arc};

use serde_json::Value;

use crate::{
    domain::notify::connector::{
        ConnectorAdapter, ConnectorAddress, ConnectorCapabilities, validate_addresses,
    },
    shared::{Error, Result},
};

/// 能建的一种连接器：类型、能力，以及配置里服务商地址的目录。
#[derive(Debug, Clone)]
pub struct SupportedConnectorType {
    pub connector_type: &'static str,
    pub capabilities: ConnectorCapabilities,
    pub addresses: Vec<ConnectorAddress>,
}

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

    /// 按类型取 adapter 并检查配置：服务商的地址只收目录里的（见 [`ConnectorAdapter::addresses`]），
    /// 其余由 adapter 自己查。保存、测试与直发前都走这里。
    pub fn validated(
        &self,
        connector_type: &str,
        config: &Value,
    ) -> Result<Arc<dyn ConnectorAdapter>> {
        let adapter = self.get(connector_type)?;

        validate_addresses(connector_type, &adapter.addresses(), config)?;
        adapter.validate_config(config)?;
        Ok(adapter)
    }

    pub fn capabilities(&self, connector_type: &str) -> Result<ConnectorCapabilities> {
        Ok(self.get(connector_type)?.capabilities())
    }

    pub fn supported_types(&self) -> Vec<SupportedConnectorType> {
        let mut supported = self
            .adapters
            .iter()
            .map(|(connector_type, adapter)| SupportedConnectorType {
                connector_type,
                capabilities: adapter.capabilities(),
                addresses: adapter.addresses(),
            })
            .collect::<Vec<_>>();

        supported.sort_by_key(|supported| supported.connector_type);
        supported
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use serde_json::json;

    use super::*;
    use crate::domain::notify::connector::{
        ConnectorDeliveryResult, NotifyChannel, NotifyMessage, NotifyTarget,
    };

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

        fn channel(&self) -> NotifyChannel {
            NotifyChannel::Im
        }

        fn addresses(&self) -> Vec<ConnectorAddress> {
            vec![ConnectorAddress::fixed(
                "apiBaseUrl",
                "https://api.example.com",
            )]
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
    fn a_config_may_only_name_an_address_of_the_catalogue() {
        let registry =
            ConnectorRegistry::new([Arc::new(FakeAdapter("slack_app")) as Arc<_>]).unwrap();
        let validated = |config: Value| registry.validated("slack_app", &config).is_ok();

        assert!(validated(json!({})));
        assert!(validated(
            json!({ "apiBaseUrl": "https://api.example.com/" })
        ));
        assert!(!validated(
            json!({ "apiBaseUrl": "https://attacker.example" })
        ));
        assert!(registry.validated("unknown", &json!({})).is_err());

        let supported = registry.supported_types();
        assert_eq!(supported[0].connector_type, "slack_app");
        assert_eq!(supported[0].addresses[0].field, "apiBaseUrl");
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
