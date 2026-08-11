// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! IAM 集成测试（in-mem）：spec
//!
//! - 第一个 user：`create_user_with_default_org` 成功 → 同时建 org、membership
//!   和数据库 purpose 指定的 bootstrap role binding
//! - 第二个 user：再次调 `create_user_with_default_org` 返 Forbidden（users 表已非空）
//! - 普通 `create_user` 仍可用

use std::{
    collections::HashMap,
    sync::{Arc, Mutex as StdMutex},
};

use async_trait::async_trait;
use molesignal::{
    app::iam::IamService,
    config::AuthSettings,
    domain::iam::{
        IamAssignedRole, IamMembership, IamMembershipRepository, Organization,
        OrganizationRepository, User, UserRepository,
    },
    shared::{Error, Result, ids::Id},
};

#[derive(Default)]
struct InMemUsers {
    inner: StdMutex<HashMap<String, User>>,
}
#[async_trait]
impl UserRepository for InMemUsers {
    async fn create(&self, user: User) -> Result<User> {
        self.inner
            .lock()
            .unwrap()
            .insert(user.id.0.clone(), user.clone());
        Ok(user)
    }
    async fn get(&self, id: &Id) -> Result<User> {
        self.inner
            .lock()
            .unwrap()
            .get(&id.0)
            .cloned()
            .ok_or_else(|| Error::not_found("user"))
    }
    async fn get_by_email(&self, email: &str) -> Result<User> {
        self.inner
            .lock()
            .unwrap()
            .values()
            .find(|u| u.email == email)
            .cloned()
            .ok_or_else(|| Error::not_found("user"))
    }
    async fn update(&self, user: User) -> Result<User> {
        self.inner
            .lock()
            .unwrap()
            .insert(user.id.0.clone(), user.clone());
        Ok(user)
    }
    async fn delete(&self, id: &Id) -> Result<()> {
        self.inner.lock().unwrap().remove(&id.0);
        Ok(())
    }
    async fn count(&self) -> Result<u64> {
        Ok(self.inner.lock().unwrap().len() as u64)
    }
    async fn list(&self) -> Result<Vec<User>> {
        Ok(self.inner.lock().unwrap().values().cloned().collect())
    }
    async fn set_status(&self, id: &Id, status: molesignal::domain::iam::UserStatus) -> Result<()> {
        if let Some(u) = self.inner.lock().unwrap().get_mut(&id.0) {
            u.status = status;
        }
        Ok(())
    }
}

#[derive(Default)]
struct InMemOrgs {
    inner: StdMutex<HashMap<String, Organization>>,
}
#[async_trait]
impl OrganizationRepository for InMemOrgs {
    async fn create(&self, org: Organization) -> Result<Organization> {
        self.inner
            .lock()
            .unwrap()
            .insert(org.id.0.clone(), org.clone());
        Ok(org)
    }
    async fn get(&self, id: &Id) -> Result<Organization> {
        self.inner
            .lock()
            .unwrap()
            .get(&id.0)
            .cloned()
            .ok_or_else(|| Error::not_found("org"))
    }
    async fn get_by_slug(&self, slug: &str) -> Result<Organization> {
        self.inner
            .lock()
            .unwrap()
            .values()
            .find(|o| o.slug == slug)
            .cloned()
            .ok_or_else(|| Error::not_found("org"))
    }
    async fn list(&self) -> Result<Vec<Organization>> {
        Ok(self.inner.lock().unwrap().values().cloned().collect())
    }
    async fn update_name(&self, id: &Id, name: String) -> Result<Organization> {
        let mut organizations = self.inner.lock().unwrap();
        let org = organizations
            .get_mut(&id.0)
            .ok_or_else(|| Error::not_found("org"))?;
        org.ensure_mutable()?;
        org.name = name;
        Ok(org.clone())
    }
    async fn set_disabled(&self, id: &Id, disabled: bool) -> Result<Organization> {
        let mut organizations = self.inner.lock().unwrap();
        let org = organizations
            .get_mut(&id.0)
            .ok_or_else(|| Error::not_found("org"))?;
        org.ensure_mutable()?;
        org.disabled = disabled;
        Ok(org.clone())
    }
    async fn delete(&self, id: &Id) -> Result<()> {
        self.inner.lock().unwrap().remove(&id.0);
        Ok(())
    }
}

#[derive(Default)]
struct InMemMemberships {
    inner: StdMutex<Vec<IamMembership>>,
    assigned: StdMutex<HashMap<(String, String), Vec<IamAssignedRole>>>,
    roles: StdMutex<HashMap<String, IamAssignedRole>>,
}
#[async_trait]
impl IamMembershipRepository for InMemMemberships {
    async fn upsert(&self, m: IamMembership, role_ids: &[Id], _actor_id: &Id) -> Result<()> {
        let assignment_key = (m.user_id.0.clone(), m.org_id.0.clone());
        let mut g = self.inner.lock().unwrap();
        if let Some(existing) = g
            .iter_mut()
            .find(|x| x.user_id == m.user_id && x.org_id == m.org_id)
        {
            *existing = m.clone();
        } else {
            g.push(m.clone());
        }
        drop(g);
        let roles = self.roles.lock().unwrap();
        let assigned = role_ids
            .iter()
            .filter_map(|role_id| roles.get(&role_id.0).cloned())
            .collect();
        self.assigned
            .lock()
            .unwrap()
            .insert(assignment_key, assigned);
        Ok(())
    }
    async fn list_for_user(&self, user_id: &Id) -> Result<Vec<IamMembership>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|m| &m.user_id == user_id)
            .cloned()
            .collect())
    }
    async fn list_for_org(&self, org_id: &Id) -> Result<Vec<IamMembership>> {
        Ok(self
            .inner
            .lock()
            .unwrap()
            .iter()
            .filter(|m| &m.org_id == org_id)
            .cloned()
            .collect())
    }
    async fn assigned_roles(&self, user_id: &Id, org_id: &Id) -> Result<Vec<IamAssignedRole>> {
        Ok(self
            .assigned
            .lock()
            .unwrap()
            .get(&(user_id.0.clone(), org_id.0.clone()))
            .cloned()
            .unwrap_or_default())
    }
    async fn role_id_for_purpose(&self, org_id: &Id, purpose: &str) -> Result<Id> {
        let key = match purpose {
            "organization_bootstrap" => "owner",
            "self_service_signup" => "viewer",
            other => return Err(Error::not_found(format!("IAM role purpose {other}"))),
        };
        let role = IamAssignedRole {
            id: Id::from_string(format!("{}-{key}", org_id.0)),
            key: key.into(),
            name: key.into(),
            builtin: true,
        };
        self.roles
            .lock()
            .unwrap()
            .insert(role.id.0.clone(), role.clone());
        Ok(role.id)
    }
    async fn remove(&self, user_id: &Id, org_id: &Id) -> Result<()> {
        let mut g = self.inner.lock().unwrap();
        g.retain(|m| !(&m.user_id == user_id && &m.org_id == org_id));
        self.assigned
            .lock()
            .unwrap()
            .remove(&(user_id.0.clone(), org_id.0.clone()));
        Ok(())
    }
}

fn make_service() -> (
    IamService,
    Arc<InMemUsers>,
    Arc<InMemOrgs>,
    Arc<InMemMemberships>,
) {
    let users: Arc<InMemUsers> = Arc::new(InMemUsers::default());
    let orgs: Arc<InMemOrgs> = Arc::new(InMemOrgs::default());
    let memberships: Arc<InMemMemberships> = Arc::new(InMemMemberships::default());
    let svc = IamService::new(
        users.clone() as Arc<dyn UserRepository>,
        orgs.clone() as Arc<dyn OrganizationRepository>,
        memberships.clone() as Arc<dyn IamMembershipRepository>,
        AuthSettings {
            deprecated_jwt_secret: None,
            token_ttl_secs: 3600,
            root_email: String::new(),
            root_password: String::new(),
        },
        // JWT signing secrets 从 DB 加载；测试里塞一把固定 secret 即可。
        vec![b"test-jwt-secret-32-bytes-min!!!!".to_vec()],
    );
    (svc, users, orgs, memberships)
}

#[tokio::test]
async fn first_user_auto_creates_org_and_owner_membership() {
    let (svc, users, orgs, memberships) = make_service();
    let (user, org) = svc
        .create_user_with_default_org("admin@example.com".into(), "Admin".into(), "secret")
        .await
        .unwrap();
    assert_eq!(users.count().await.unwrap(), 1);
    let listed_orgs = orgs.list().await.unwrap();
    assert_eq!(listed_orgs.len(), 1);
    assert_eq!(listed_orgs[0].id, org.id);
    let m = memberships.list_for_user(&user.id).await.unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].org_id, org.id);
    let roles = memberships.assigned_roles(&user.id, &org.id).await.unwrap();
    assert_eq!(roles.len(), 1);
    assert_eq!(roles[0].key, "owner");
}

#[tokio::test]
async fn second_call_is_forbidden_when_users_table_not_empty() {
    let (svc, _, _, _) = make_service();
    svc.create_user_with_default_org("first@example.com".into(), "First".into(), "p1")
        .await
        .unwrap();
    let err = svc
        .create_user_with_default_org("second@example.com".into(), "Second".into(), "p2")
        .await
        .unwrap_err();
    assert_eq!(err.http_status_code(), 403, "expected Forbidden");
}

#[tokio::test]
async fn ordinary_create_user_still_works_after_first() {
    let (svc, users, _, _) = make_service();
    svc.create_user_with_default_org("admin@example.com".into(), "Admin".into(), "secret")
        .await
        .unwrap();
    let _ = svc
        .create_user("normal@example.com".into(), "Normal".into(), "p")
        .await
        .unwrap();
    assert_eq!(users.count().await.unwrap(), 2);
}
