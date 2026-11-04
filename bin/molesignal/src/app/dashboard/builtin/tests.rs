// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use parking_lot::Mutex;

use super::*;
use crate::{
    app::dashboard::DashboardService,
    domain::{
        dashboard::{
            Dashboard, Folder,
            repositories::{DashboardRepository, FolderRepository},
        },
        rum::{
            RumApplication, RumApplicationPatch, RumApplicationRepository, RumApplicationSettings,
            RumPlatform,
        },
    },
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Default)]
struct Dashboards {
    values: Mutex<HashMap<Id, Dashboard>>,
}

#[async_trait]
impl DashboardRepository for Dashboards {
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

    async fn update_if_version(&self, dashboard: Dashboard, _: u32) -> Result<Dashboard> {
        self.update(dashboard).await
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

    async fn list(&self, org_id: &Id, _: Option<&Id>) -> Result<Vec<Dashboard>> {
        Ok(self
            .values
            .lock()
            .values()
            .filter(|dashboard| &dashboard.org_id == org_id)
            .cloned()
            .collect())
    }

    async fn delete(&self, id: &Id) -> Result<()> {
        self.values.lock().remove(id);
        Ok(())
    }
}

struct NoFolders;

#[async_trait]
impl FolderRepository for NoFolders {
    async fn create(&self, folder: Folder) -> Result<Folder> {
        Ok(folder)
    }

    async fn get_by_id(&self, _: &Id) -> Result<Folder> {
        Err(Error::not_found("folder"))
    }

    async fn get(&self, _: &Id, _: &Id) -> Result<Folder> {
        Err(Error::not_found("folder"))
    }

    async fn list(&self, _: &Id) -> Result<Vec<Folder>> {
        Ok(Vec::new())
    }

    async fn update(&self, folder: Folder) -> Result<Folder> {
        Ok(folder)
    }

    async fn delete(&self, _: &Id) -> Result<()> {
        Ok(())
    }
}

struct Applications(Vec<RumPlatform>);

#[async_trait]
impl RumApplicationRepository for Applications {
    async fn create(&self, application: RumApplication) -> Result<RumApplication> {
        Ok(application)
    }

    async fn get(&self, _: &Id, _: &str) -> Result<RumApplication> {
        Err(Error::not_found("application"))
    }

    async fn list(&self, org_id: &Id) -> Result<Vec<RumApplication>> {
        Ok(self
            .0
            .iter()
            .enumerate()
            .map(|(index, platform)| RumApplication {
                org_id: org_id.clone(),
                id: format!("app-{index}"),
                name: format!("app {index}"),
                platform: *platform,
                settings: RumApplicationSettings::default(),
                created_by: None,
                created_at: TimestampMicros(1),
                updated_at: TimestampMicros(1),
            })
            .collect())
    }

    async fn update(&self, _: &Id, _: &str, _: RumApplicationPatch) -> Result<RumApplication> {
        Err(Error::not_found("application"))
    }

    async fn delete(&self, _: &Id, _: &str) -> Result<()> {
        Ok(())
    }
}

fn service() -> (DashboardService, Arc<Dashboards>) {
    let dashboards = Arc::new(Dashboards::default());

    (
        DashboardService::new(dashboards.clone(), Arc::new(NoFolders)),
        dashboards,
    )
}

#[tokio::test]
async fn a_dashboard_is_created_once_and_then_found_current() {
    let (service, dashboards) = service();
    let org = Id::from_string("org-1");

    for dashboard in RumDashboard::ALL {
        assert_eq!(
            service.install_rum(&org, dashboard).await.unwrap(),
            Installed::Created
        );

        assert_eq!(
            service.install_rum(&org, dashboard).await.unwrap(),
            Installed::Current
        );
    }

    let stored = dashboards.list(&org, None).await.unwrap();
    let mut uids = stored.iter().map(|d| d.uid.as_str()).collect::<Vec<_>>();

    uids.sort_unstable();

    assert_eq!(uids, ["rum-browser", "rum-mobile"]);

    for dashboard in stored {
        assert_eq!(dashboard.version, 1);
        assert_eq!(dashboard.model["id"], dashboard.id.0.as_str());
        assert_eq!(dashboard.model["version"], 1);
        assert_eq!(dashboard.created_by, Id::from_string("molesignal-system"));
        assert_eq!(dashboard.tags, ["内置", "rum"]);
        assert!(dashboard.folder_id.is_none());
    }
}

#[tokio::test]
async fn two_organizations_never_share_a_dashboard() {
    let (service, dashboards) = service();
    let first = Id::from_string("org-1");
    let second = Id::from_string("org-2");

    service
        .install_rum(&first, RumDashboard::Browser)
        .await
        .unwrap();
    service
        .install_rum(&second, RumDashboard::Browser)
        .await
        .unwrap();

    let in_first = dashboards.list(&first, None).await.unwrap();
    let in_second = dashboards.list(&second, None).await.unwrap();

    assert_eq!(in_first.len(), 1);
    assert_eq!(in_second.len(), 1);
    assert_ne!(in_first[0].id, in_second[0].id);
}

#[tokio::test]
async fn a_dashboard_of_an_older_version_is_written_again_where_it_was_put() {
    let (service, dashboards) = service();
    let org = Id::from_string("org-1");

    service
        .install_rum(&org, RumDashboard::Mobile)
        .await
        .unwrap();

    let mut stored = dashboards.get_by_uid(&org, "rum-mobile").await.unwrap();

    stored.model["extensions"]["builtin"]["version"] = serde_json::json!(0);
    stored.model["title"] = serde_json::json!("changed");
    stored.title = "changed".into();
    stored.folder_id = Some(Id::from_string("folder-1"));
    dashboards.update(stored.clone()).await.unwrap();

    assert_eq!(
        service
            .install_rum(&org, RumDashboard::Mobile)
            .await
            .unwrap(),
        Installed::Updated
    );

    let again = dashboards.get_by_uid(&org, "rum-mobile").await.unwrap();

    assert_eq!(again.id, stored.id);
    assert_eq!(again.title, RumDashboard::Mobile.title());
    assert_eq!(again.model["title"], RumDashboard::Mobile.title());
    assert_eq!(again.version, stored.version + 1);
    assert_eq!(again.model["version"], again.version);
    assert_eq!(again.folder_id, Some(Id::from_string("folder-1")));
    assert_eq!(again.model["folderId"], "folder-1");
    assert_eq!(again.created_at, stored.created_at);
}

#[tokio::test]
async fn an_organization_gets_the_dashboards_of_the_platforms_it_has_applications_of() {
    let (service, _) = service();
    let org = Id::from_string("org-1");

    let installed = |platforms: Vec<RumPlatform>| {
        let service = &service;
        let org = &org;

        async move {
            service
                .install_rum_for_applications(org, &Applications(platforms))
                .await
                .unwrap()
                .into_iter()
                .map(|(dashboard, _)| dashboard)
                .collect::<Vec<_>>()
        }
    };

    assert!(installed(Vec::new()).await.is_empty());
    assert!(installed(vec![RumPlatform::Other]).await.is_empty());

    assert_eq!(
        installed(vec![RumPlatform::Browser, RumPlatform::Browser]).await,
        [RumDashboard::Browser]
    );

    assert_eq!(
        installed(vec![
            RumPlatform::Ios,
            RumPlatform::Android,
            RumPlatform::Browser
        ])
        .await,
        [RumDashboard::Mobile, RumDashboard::Browser]
    );
}

#[tokio::test]
async fn a_dashboard_somebody_else_made_with_the_uid_is_left_alone() {
    let (service, dashboards) = service();
    let org = Id::from_string("org-1");

    let theirs = Dashboard {
        id: Id::from_string("theirs"),
        org_id: org.clone(),
        folder_id: None,
        uid: "rum-browser".into(),
        title: "Mine".into(),
        tags: Vec::new(),
        model: serde_json::json!({ "title": "Mine" }),
        version: 3,
        created_at: TimestampMicros(1),
        updated_at: TimestampMicros(1),
        created_by: Id::from_string("user-1"),
        updated_by: Id::from_string("user-1"),
    };

    dashboards.create(theirs).await.unwrap();

    assert_eq!(
        service
            .install_rum(&org, RumDashboard::Browser)
            .await
            .unwrap(),
        Installed::Taken
    );

    let kept = dashboards.get(&Id::from_string("theirs")).await.unwrap();

    assert_eq!(kept.title, "Mine");
    assert_eq!(kept.version, 3);
    assert_eq!(kept.model["title"], "Mine");
}
