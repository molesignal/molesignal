// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::collections::HashMap;

use async_trait::async_trait;
use parking_lot::Mutex;

use super::*;

#[derive(Default)]
struct Views(Mutex<HashMap<Id, SavedView>>);

#[async_trait]
impl SavedViewRepository for Views {
    async fn create(&self, view: SavedView) -> Result<SavedView> {
        let mut rows = self.0.lock();
        if rows.contains_key(&view.id) {
            return Err(Error::conflict("exists"));
        }
        rows.insert(view.id.clone(), view.clone());
        Ok(view)
    }
    async fn update(&self, view: SavedView) -> Result<SavedView> {
        self.0.lock().insert(view.id.clone(), view.clone());
        Ok(view)
    }
    async fn get_by_id(&self, id: &Id) -> Result<SavedView> {
        self.0
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| Error::not_found("view"))
    }
    async fn get(&self, org: &Id, id: &Id) -> Result<SavedView> {
        let view = self.get_by_id(id).await?;
        if &view.org_id != org {
            return Err(Error::not_found("view"));
        }
        Ok(view)
    }
    async fn list(&self, org: &Id, _: bool) -> Result<Vec<SavedView>> {
        Ok(self
            .0
            .lock()
            .values()
            .filter(|v| &v.org_id == org)
            .cloned()
            .collect())
    }
    async fn delete(&self, _: &Id, id: &Id) -> Result<()> {
        self.0.lock().remove(id);
        Ok(())
    }
}

#[tokio::test]
async fn retries_repair_partial_installs_without_overwriting_edits_or_crossing_orgs() {
    let repo = Views::default();
    let org = Id::from_string("one");
    let user = Id::from_string("member");
    assert!(!views_ready(&repo, &org).await.unwrap());
    let ids = install_views(&repo, &org, &user).await.unwrap();
    assert_eq!(ids.len(), 4);
    assert!(views_ready(&repo, &org).await.unwrap());

    let mut edited = repo.get(&org, &ids[0]).await.unwrap();
    edited.statement = "SELECT message FROM sample_app_logs".into();
    repo.update(edited.clone()).await.unwrap();
    repo.delete(&org, &ids[1]).await.unwrap();
    assert!(!views_ready(&repo, &org).await.unwrap());
    assert_eq!(install_views(&repo, &org, &user).await.unwrap(), ids);
    assert_eq!(
        repo.get(&org, &ids[0]).await.unwrap().statement,
        edited.statement
    );
    assert!(views_ready(&repo, &org).await.unwrap());

    let other = Id::from_string("two");
    let other_ids = install_views(&repo, &other, &user).await.unwrap();
    assert!(other_ids.iter().all(|id| !ids.contains(id)));
    assert!(repo.get(&other, &ids[0]).await.is_err());
}
