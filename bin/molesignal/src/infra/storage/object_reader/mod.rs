// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Catalog-aware object reads and the shared remote range-block cache.

use std::sync::Arc;

use dashmap::DashMap;
use object_store::ObjectStore;

use crate::{
    config::ObjectCacheSettings,
    domain::storage::{DataSegment, StoredObject},
    shared::{Error, Result, ids::Id},
};

mod block_cache;
mod metrics;
mod store;

use block_cache::RangeBlockCache;
use store::CachedObjectStore;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RegisteredObject {
    pub(super) organization_id: Id,
    pub(super) object: StoredObject,
}

#[derive(Default)]
pub(super) struct ObjectRegistry {
    objects: DashMap<String, RegisteredObject>,
}

impl ObjectRegistry {
    fn register(&self, organization_id: &Id, object: &StoredObject) -> Result<()> {
        let key = object.key.as_str().to_owned();
        let registered = RegisteredObject {
            organization_id: organization_id.clone(),
            object: object.clone(),
        };
        if let Some(existing) = self.objects.get(&key) {
            if *existing == registered {
                return Ok(());
            }
            return Err(Error::internal(format!(
                "immutable object key {key} was registered with different Catalog identity"
            )));
        }
        self.objects.insert(key, registered);
        Ok(())
    }

    fn get(&self, key: &str) -> Option<RegisteredObject> {
        self.objects.get(key).map(|entry| entry.clone())
    }
}

/// The only object-store handle injected into readers. Writers keep the origin handle.
pub struct ObjectReader {
    origin: Arc<dyn ObjectStore>,
    read_store: Arc<dyn ObjectStore>,
    registry: Arc<ObjectRegistry>,
    cache_enabled: bool,
}

impl ObjectReader {
    pub fn build(
        origin: Arc<dyn ObjectStore>,
        backend: &str,
        settings: &ObjectCacheSettings,
    ) -> Result<Arc<Self>> {
        let registry = Arc::new(ObjectRegistry::default());
        let cache_enabled = backend != "local" && settings.is_effectively_enabled();
        let read_store: Arc<dyn ObjectStore> = if cache_enabled {
            let cache = Arc::new(RangeBlockCache::new(settings)?);
            Arc::new(CachedObjectStore::new(
                origin.clone(),
                registry.clone(),
                cache,
            ))
        } else {
            origin.clone()
        };
        if backend == "local" {
            tracing::info!(object_backend = "local", object_cache = "bypassed");
        } else if cache_enabled {
            tracing::info!(
                object_backend = "remote",
                object_cache = "enabled",
                object_cache_mode = "block",
                object_cache_block_size = settings.block_size_bytes,
            );
        } else {
            tracing::info!(object_backend = "remote", object_cache = "disabled");
        }
        Ok(Arc::new(Self {
            origin,
            read_store,
            registry,
            cache_enabled,
        }))
    }

    pub fn store(&self) -> Arc<dyn ObjectStore> {
        self.read_store.clone()
    }

    pub fn origin(&self) -> Arc<dyn ObjectStore> {
        self.origin.clone()
    }

    pub fn cache_enabled(&self) -> bool {
        self.cache_enabled
    }

    pub fn register(&self, organization_id: &Id, object: &StoredObject) -> Result<()> {
        self.registry.register(organization_id, object)
    }

    pub fn register_segment(&self, organization_id: &Id, segment: &DataSegment) -> Result<()> {
        self.register(organization_id, &segment.primary.object)?;
        for artifact in &segment.auxiliaries {
            self.register(organization_id, &artifact.object)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use object_store::memory::InMemory;

    use super::*;
    use crate::domain::storage::{ObjectChecksum, ObjectKey};

    #[test]
    fn local_store_is_bypassed() {
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let reader =
            ObjectReader::build(store.clone(), "local", &ObjectCacheSettings::default()).unwrap();
        assert!(!reader.cache_enabled());
        assert!(Arc::ptr_eq(&reader.store(), &store));
    }

    #[test]
    fn immutable_registration_rejects_identity_change() {
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let settings = ObjectCacheSettings {
            enabled: false,
            ..ObjectCacheSettings::default()
        };
        let reader = ObjectReader::build(store, "s3", &settings).unwrap();
        let object = StoredObject {
            key: ObjectKey::from_string("v1/artifacts/one"),
            size_bytes: 1,
            checksum: ObjectChecksum::from_string("b3:one"),
            etag: None,
        };
        reader.register(&Id::from_string("org-a"), &object).unwrap();
        let mut changed = object;
        changed.size_bytes = 2;
        assert!(
            reader
                .register(&Id::from_string("org-a"), &changed)
                .is_err()
        );
    }
}
