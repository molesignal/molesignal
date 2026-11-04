// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 分区域的连接器（阿里云、AWS）能用的区域：由它的适配器向厂商查，不写死。要凭证的用还在填的
//! 配置；编辑已有连接器时，配置里 `"***"` 的敏感字段取存着的值。

use serde_json::Value;

use super::{
    super::config::{merge_masked_config, normalize_object},
    NotifyService,
};
use crate::{
    domain::notify::connector::{ConnectorRegion, ConnectorStatus, NotifyConnector},
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

/// 列一种连接器能用的区域：还在填的配置，编辑已有连接器时是它的 ID，以及区域名称的语言。
#[derive(Debug, Clone)]
pub struct ConnectorRegionsQuery {
    pub connector_type: String,
    pub config: Option<Value>,
    pub connector_id: Option<Id>,
    pub language: String,
}

impl NotifyService {
    pub async fn connector_regions(
        &self,
        organization_id: &Id,
        query: ConnectorRegionsQuery,
    ) -> Result<Vec<ConnectorRegion>> {
        let adapter = self.registry.get(&query.connector_type)?;

        let stored = match &query.connector_id {
            Some(id) => Some(self.connectors.get(organization_id, id).await?),
            None => None,
        };

        if let Some(stored) = &stored
            && stored.connector_type != query.connector_type
        {
            return Err(Error::invalid(
                "the connector is of another type than the regions asked for",
            ));
        }

        let config = match (&stored, query.config) {
            (Some(stored), Some(config)) => {
                merge_masked_config(&stored.config, &normalize_object(config, "config")?)
            }
            (Some(stored), None) => stored.config.clone(),
            (None, Some(config)) => normalize_object(config, "config")?,
            (None, None) => Value::Object(Default::default()),
        };

        let now = TimestampMicros::now();

        // 还没保存的连接器也要一个身份：AWS 扮演角色时用组织的 External ID。
        let connector = NotifyConnector {
            id: stored
                .as_ref()
                .map_or_else(Id::new, |stored| stored.id.clone()),
            organization_id: organization_id.clone(),
            name: stored
                .as_ref()
                .map(|stored| stored.name.clone())
                .unwrap_or_default(),
            connector_type: query.connector_type,
            config,
            capabilities: adapter.capabilities(),
            enabled: true,
            status: ConnectorStatus::Unknown,
            last_tested_at: None,
            last_test_status: None,
            last_test_error: None,
            created_at: now,
            updated_at: now,
        };

        adapter.regions(&connector, &query.language).await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use serde_json::json;

    use super::*;
    use crate::{
        app::notify::{
            ConnectorRegistry,
            engine::tests::{MemoryStore, connector},
        },
        domain::notify::{
            connector::{
                ConnectorAdapter, ConnectorCapabilities, ConnectorDeliveryResult, NotifyChannel,
                NotifyMessage, NotifyTarget,
            },
            repositories::{NotifyConnectorRepository, NotifyRouteReferenceRepository},
        },
    };

    /// 记下拿到的配置与语言，列出一个区域。
    #[derive(Default)]
    struct Regional {
        asked: Mutex<Vec<(Value, String)>>,
    }

    #[async_trait]
    impl ConnectorAdapter for Regional {
        fn connector_type(&self) -> &'static str {
            "fake"
        }

        fn capabilities(&self) -> ConnectorCapabilities {
            ConnectorCapabilities::default()
        }

        fn channel(&self) -> NotifyChannel {
            NotifyChannel::Sms
        }

        fn validate_config(&self, _config: &Value) -> Result<()> {
            Ok(())
        }

        fn validate_target(&self, _target: &NotifyTarget) -> Result<()> {
            Ok(())
        }

        async fn regions(
            &self,
            connector: &NotifyConnector,
            language: &str,
        ) -> Result<Vec<ConnectorRegion>> {
            self.asked
                .lock()
                .unwrap()
                .push((connector.config.clone(), language.to_string()));

            Ok(vec![ConnectorRegion {
                id: "eu-west-1".into(),
                name: "eu-west-1".into(),
                api_base_url: "https://sms-voice.eu-west-1.amazonaws.com".into(),
            }])
        }

        async fn send(
            &self,
            _config: &Value,
            _target: &NotifyTarget,
            _message: &NotifyMessage,
        ) -> Result<ConnectorDeliveryResult> {
            Err(Error::unavailable("regions only"))
        }
    }

    struct NoReferences;

    #[async_trait]
    impl NotifyRouteReferenceRepository for NoReferences {
        async fn count_for_connector(&self, _org: &Id, _connector: &Id) -> Result<u64> {
            Ok(0)
        }
    }

    #[tokio::test]
    async fn an_edited_connector_lists_regions_with_its_stored_secrets() {
        let store = Arc::new(MemoryStore::default());
        let adapter = Arc::new(Regional::default());
        let org = Id::from_string("org-a");

        let mut stored = connector(&org);
        stored.config = json!({ "region": "us-east-1", "secretKey": "kept-secret" });
        NotifyConnectorRepository::create(store.as_ref(), stored)
            .await
            .unwrap();

        let registry =
            ConnectorRegistry::new([adapter.clone() as Arc<dyn ConnectorAdapter>]).unwrap();

        let service = NotifyService::new(
            store.clone(),
            store.clone(),
            store.clone(),
            store.clone(),
            Arc::new(NoReferences),
            store.clone(),
            Arc::new(registry),
        );

        let regions = service
            .connector_regions(
                &org,
                ConnectorRegionsQuery {
                    connector_type: "fake".into(),
                    config: Some(json!({ "region": "eu-west-1", "secretKey": "***" })),
                    connector_id: Some(Id::from_string("connector-a")),
                    language: "zh-cn".into(),
                },
            )
            .await
            .unwrap();

        assert_eq!(regions[0].id, "eu-west-1");
        assert_eq!(
            adapter.asked.lock().unwrap()[0],
            (
                json!({ "region": "eu-west-1", "secretKey": "kept-secret" }),
                "zh-cn".to_string()
            )
        );

        // 新的连接器用填的配置原样。
        service
            .connector_regions(
                &org,
                ConnectorRegionsQuery {
                    connector_type: "fake".into(),
                    config: Some(json!({ "secretKey": "typed" })),
                    connector_id: None,
                    language: "en-us".into(),
                },
            )
            .await
            .unwrap();

        assert_eq!(
            adapter.asked.lock().unwrap()[1].0,
            json!({ "secretKey": "typed" })
        );

        let unknown = service
            .connector_regions(
                &org,
                ConnectorRegionsQuery {
                    connector_type: "aws_sms".into(),
                    config: None,
                    connector_id: None,
                    language: "en-us".into(),
                },
            )
            .await;

        assert!(unknown.is_err());
    }
}
