// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::Value;

use super::*;
use crate::domain::dashboard::{Folder, repositories::FolderRepository};

const VALID_MODEL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/dashboard/fixtures/valid/dashboard-v2-nested.json"
));
const DUPLICATE_IDS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/contracts/dashboard/fixtures/invalid/dashboard-v2-duplicate-id.json"
));

#[derive(Default)]
struct MemoryDashboards {
    values: Mutex<HashMap<Id, Dashboard>>,
}

#[async_trait]
impl DashboardRepository for MemoryDashboards {
    async fn create(&self, dashboard: Dashboard) -> Result<Dashboard> {
        self.values
            .lock()
            .insert(dashboard.id.clone(), dashboard.clone());
        Ok(dashboard)
    }

    async fn update(&self, dashboard: Dashboard) -> Result<Dashboard> {
        self.values
            .lock()
            .insert(dashboard.id.clone(), dashboard.clone());
        Ok(dashboard)
    }

    async fn get(&self, id: &Id) -> Result<Dashboard> {
        self.values
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| Error::not_found("dashboard"))
    }

    async fn get_by_uid(&self, org_id: &Id, uid: &str) -> Result<Dashboard> {
        self.values
            .lock()
            .values()
            .find(|dashboard| &dashboard.org_id == org_id && dashboard.uid == uid)
            .cloned()
            .ok_or_else(|| Error::not_found("dashboard"))
    }

    async fn list(&self, org_id: &Id, folder_id: Option<&Id>) -> Result<Vec<Dashboard>> {
        Ok(self
            .values
            .lock()
            .values()
            .filter(|dashboard| {
                &dashboard.org_id == org_id
                    && folder_id.is_none_or(|folder| dashboard.folder_id.as_ref() == Some(folder))
            })
            .cloned()
            .collect())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.values
            .lock()
            .remove(id)
            .map(|_| ())
            .ok_or_else(|| Error::not_found("dashboard"))
    }
}

#[derive(Default)]
struct MemoryFolders;

#[async_trait]
impl FolderRepository for MemoryFolders {
    async fn create(&self, folder: Folder) -> Result<Folder> {
        Ok(folder)
    }

    async fn get_by_id(&self, _id: &Id) -> Result<Folder> {
        Err(Error::not_found("folder"))
    }

    async fn get(&self, _org_id: &Id, _id: &Id) -> Result<Folder> {
        Err(Error::not_found("folder"))
    }

    async fn list(&self, _org_id: &Id) -> Result<Vec<Folder>> {
        Ok(Vec::new())
    }

    async fn update(&self, folder: Folder) -> Result<Folder> {
        Ok(folder)
    }

    async fn delete(&self, _id: &Id) -> Result<()> {
        Ok(())
    }
}

fn service() -> (DashboardService, Arc<MemoryDashboards>) {
    let dashboards = Arc::new(MemoryDashboards::default());
    (
        DashboardService::new(dashboards.clone(), Arc::new(MemoryFolders)),
        dashboards,
    )
}

fn valid_model() -> Value {
    serde_json::from_str(VALID_MODEL).unwrap()
}

#[tokio::test]
async fn create_injects_missing_uid_and_accepts_current_model() {
    let (service, repository) = service();
    let mut model = valid_model();
    model.as_object_mut().unwrap().remove("uid");

    let created = service
        .create(Id::new(), None, Id::new(), model)
        .await
        .unwrap();

    assert!(!created.uid.is_empty());
    assert_eq!(created.model["uid"], created.uid);
    assert_eq!(repository.values.lock().len(), 1);
}

#[tokio::test]
async fn invalid_models_never_mutate_the_repository() {
    let (service, repository) = service();
    let duplicate: Value = serde_json::from_str(DUPLICATE_IDS).unwrap();
    let error = service
        .create(Id::new(), None, Id::new(), duplicate)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Validation { .. }));

    let mut out_of_grid = valid_model();
    out_of_grid["elements"][0]["gridPos"]["w"] = Value::from(25);
    assert!(
        service
            .create(Id::new(), None, Id::new(), out_of_grid)
            .await
            .is_err()
    );

    let mut incompatible = valid_model();
    incompatible["elements"][0]["visualization"]["type"] = Value::from("logs");
    assert!(
        service
            .create(Id::new(), None, Id::new(), incompatible)
            .await
            .is_err()
    );
    assert!(repository.values.lock().is_empty());
}

#[tokio::test]
async fn failed_update_is_immutable_and_success_preserves_uid() {
    let (service, repository) = service();
    let created = service
        .create(Id::new(), None, Id::new(), valid_model())
        .await
        .unwrap();
    let original_updated_at = created.updated_at;

    let mut invalid = created.model.clone();
    invalid["elements"][0]["gridPos"]["x"] = Value::from(24);
    assert!(
        service
            .update_model(created.clone(), None, Id::new(), invalid)
            .await
            .is_err()
    );
    let unchanged = repository.get(&created.id).await.unwrap();
    assert_eq!(unchanged.version, 1);
    assert_eq!(unchanged.updated_at, original_updated_at);

    let mut update = created.model.clone();
    update["uid"] = Value::from("model-attempted-uid-change");
    update["title"] = Value::from("Updated title");
    let saved = service
        .update_model(created.clone(), None, Id::new(), update)
        .await
        .unwrap();
    assert_eq!(saved.uid, created.uid);
    assert_eq!(saved.model["uid"], created.uid);
    assert_eq!(saved.version, 2);
}

#[tokio::test]
async fn grafana_import_mode_preserves_vendor_extensions() {
    let (service, _) = service();
    let mut model = valid_model();
    model["weirdCustomKey"] = Value::from("preserved");
    model["elements"][0]["extraVendorKey"] = Value::from(42);

    let created = service
        .create_grafana_import(Id::new(), None, Id::new(), model)
        .await
        .unwrap();
    assert_eq!(created.model["weirdCustomKey"], "preserved");
    assert_eq!(created.model["elements"][0]["extraVendorKey"], 42);
}
