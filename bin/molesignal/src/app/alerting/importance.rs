// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 组织的优先级与紧急度档位（见 [`crate::domain::alerting::importance`]）：读、改。
//!
//! 开事件、升级、发通知时都要看档位，所以每个组织的档位在本节点缓存 [`CACHE_FOR`]；本节点改了立即
//! 换掉，别的节点最多晚这么久。读不出时按内置的档位算，不挡住开事件和通知。

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use crate::{
    domain::alerting::importance::{ImportanceLevels, ImportanceRepository, StoredImportance},
    shared::{Result, ids::Id, time::TimestampMicros},
};

/// 一个组织的档位缓存多久。
const CACHE_FOR: Duration = Duration::from_secs(30);

pub struct ImportanceService {
    repository: Arc<dyn ImportanceRepository>,
    cache: Mutex<HashMap<Id, (Instant, Arc<ImportanceLevels>)>>,
}

impl ImportanceService {
    pub fn new(repository: Arc<dyn ImportanceRepository>) -> Arc<Self> {
        Arc::new(Self {
            repository,
            cache: Mutex::new(HashMap::new()),
        })
    }

    /// 这个组织的档位；没改过、或读不出时是内置的。
    pub async fn levels(&self, org_id: &Id) -> Arc<ImportanceLevels> {
        if let Some((at, levels)) = self
            .cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(org_id)
            && at.elapsed() < CACHE_FOR
        {
            return levels.clone();
        }

        let levels = match self.repository.get(org_id).await {
            Ok(stored) => Arc::new(stored.map(|stored| stored.levels).unwrap_or_default()),
            Err(error) => {
                tracing::warn!(%error, org_id = %org_id, "importance levels unreadable; the built-in ones apply");
                return Arc::new(ImportanceLevels::default());
            }
        };

        self.remember(org_id, levels.clone());
        levels
    }

    /// 存下来的档位和谁、什么时候改的；没改过时是内置的，没有修改人。
    pub async fn stored(&self, org_id: &Id) -> Result<StoredImportance> {
        Ok(self
            .repository
            .get(org_id)
            .await?
            .unwrap_or_else(|| StoredImportance {
                levels: ImportanceLevels::default(),
                updated_by: None,
                updated_at: TimestampMicros(0),
            }))
    }

    /// 检查后存下，本节点立即用新的。
    pub async fn save(
        &self,
        org_id: &Id,
        mut levels: ImportanceLevels,
        updated_by: Option<Id>,
    ) -> Result<StoredImportance> {
        levels.normalize()?;

        let stored = StoredImportance {
            levels,
            updated_by,
            updated_at: TimestampMicros::now(),
        };

        self.repository.save(org_id, &stored).await?;
        self.remember(org_id, Arc::new(stored.levels.clone()));
        Ok(stored)
    }

    fn remember(&self, org_id: &Id, levels: Arc<ImportanceLevels>) {
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(org_id.clone(), (Instant::now(), levels));
    }
}

/// 没接上档位服务时（测试、工具）按内置的档位算。
pub async fn levels_of(
    service: Option<&Arc<ImportanceService>>,
    org_id: &Id,
) -> Arc<ImportanceLevels> {
    match service {
        Some(service) => service.levels(org_id).await,
        None => Arc::new(ImportanceLevels::default()),
    }
}
