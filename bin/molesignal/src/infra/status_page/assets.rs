// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Object-store cleanup for archived Status Page assets.

use std::sync::Arc;

use futures::StreamExt as _;
use object_store::{ObjectStore, ObjectStoreExt as _, path::Path as ObjPath};

use crate::{
    domain::status_page::StatusPage,
    shared::{Error, Result},
};

pub struct StatusPageAssetCleaner {
    object_store: Arc<dyn ObjectStore>,
}

impl StatusPageAssetCleaner {
    pub fn new(object_store: Arc<dyn ObjectStore>) -> Self {
        Self { object_store }
    }

    pub async fn delete_page_assets(&self, page: &StatusPage) -> Result<u64> {
        let prefix = ObjPath::from(format!("status-page-logos/{}/{}/", page.org_id, page.id));
        let mut objects = self.object_store.list(Some(&prefix));
        let mut deleted = 0_u64;
        while let Some(object) = objects.next().await {
            let object = object
                .map_err(|error| Error::internal(format!("status page asset list: {error}")))?;
            self.object_store
                .delete(&object.location)
                .await
                .map_err(|error| Error::internal(format!("status page asset delete: {error}")))?;
            deleted = deleted.saturating_add(1);
        }
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use object_store::{ObjectStoreExt as _, PutPayload, memory::InMemory};

    use super::*;
    use crate::{
        domain::status_page::{StatusPageLifecycle, StatusPageVisibility},
        shared::{ids::Id, time::TimestampMicros},
    };

    fn page() -> StatusPage {
        StatusPage {
            id: Id("page-one".into()),
            org_id: Id("org-one".into()),
            name: "Acme".into(),
            slug: "acme".into(),
            logo_url: None,
            brand_color: "#4F46E5".into(),
            custom_domain: None,
            timezone: "UTC".into(),
            language: "en-us".into(),
            languages: vec!["en-us".into()],
            history_days: 90,
            delivery_retention_days: 90,
            private_session_days: 7,
            visibility: StatusPageVisibility::Public,
            lifecycle: StatusPageLifecycle::Archived,
            archived_at: Some(TimestampMicros(1)),
            purge_after: Some(TimestampMicros(2)),
            created_at: TimestampMicros(0),
            updated_at: TimestampMicros(1),
        }
    }

    #[tokio::test]
    async fn cleanup_is_scoped_to_one_organization_and_page() {
        let store = Arc::new(InMemory::new());
        let owned = ObjPath::from("status-page-logos/org-one/page-one/logo.png");
        let sibling = ObjPath::from("status-page-logos/org-one/page-two/logo.png");
        store
            .put(&owned, PutPayload::from_static(b"owned"))
            .await
            .unwrap();
        store
            .put(&sibling, PutPayload::from_static(b"sibling"))
            .await
            .unwrap();

        let cleaner = StatusPageAssetCleaner::new(store.clone());
        assert_eq!(cleaner.delete_page_assets(&page()).await.unwrap(), 1);
        assert!(store.get(&owned).await.is_err());
        assert!(store.get(&sibling).await.is_ok());
    }
}
