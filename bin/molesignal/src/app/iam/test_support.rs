// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 单测共用：一个只签发和校验 token 的 [`IamService`]，不读任何仓储。

use std::sync::Arc;

use async_trait::async_trait;
use base64::Engine as _;

use super::IamService;
use crate::{
    config::AuthSettings,
    domain::iam::{
        IamAssignedRole, IamMembership, IamMembershipRepository, Organization,
        OrganizationRepository, User, UserRepository, UserStatus,
    },
    shared::{Error, Result, ids::Id},
};

/// 会话有效期为 `token_ttl_secs` 的 [`IamService`]。
pub(crate) fn iam_service(token_ttl_secs: u64) -> IamService {
    IamService::new(
        Arc::new(NoDirectory),
        Arc::new(NoDirectory),
        Arc::new(NoDirectory),
        AuthSettings {
            deprecated_jwt_secret: None,
            token_ttl_secs,
            root_email: String::new(),
            root_password: String::new(),
        },
        vec![b"iam-test-support-secret".to_vec()],
    )
}

/// token 从签发到过期的秒数（`exp - iat`）。
pub(crate) fn token_lifetime_secs(token: &str) -> u64 {
    let payload = token.split('.').nth(1).expect("a JWT has three parts");

    let claims: serde_json::Value = serde_json::from_slice(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload)
            .expect("JWT payload is base64url"),
    )
    .expect("JWT payload is JSON");

    claims["exp"].as_u64().expect("exp") - claims["iat"].as_u64().expect("iat")
}

/// 没有任何用户、组织和成员关系的目录。
struct NoDirectory;

fn empty() -> Error {
    Error::not_found("the test directory is empty")
}

#[async_trait]
impl UserRepository for NoDirectory {
    async fn create(&self, _: User) -> Result<User> {
        Err(empty())
    }
    async fn get(&self, _: &Id) -> Result<User> {
        Err(empty())
    }
    async fn get_by_email(&self, _: &str) -> Result<User> {
        Err(empty())
    }
    async fn update(&self, _: User) -> Result<User> {
        Err(empty())
    }
    async fn delete(&self, _: &Id) -> Result<()> {
        Err(empty())
    }
    async fn count(&self) -> Result<u64> {
        Ok(0)
    }
    async fn list(&self) -> Result<Vec<User>> {
        Ok(Vec::new())
    }
    async fn set_status(&self, _: &Id, _: UserStatus) -> Result<()> {
        Err(empty())
    }
}

#[async_trait]
impl OrganizationRepository for NoDirectory {
    async fn create(&self, _: Organization) -> Result<Organization> {
        Err(empty())
    }
    async fn get(&self, _: &Id) -> Result<Organization> {
        Err(empty())
    }
    async fn get_by_slug(&self, _: &str) -> Result<Organization> {
        Err(empty())
    }
    async fn list(&self) -> Result<Vec<Organization>> {
        Ok(Vec::new())
    }
    async fn update_name(&self, _: &Id, _: String) -> Result<Organization> {
        Err(empty())
    }
    async fn set_disabled(&self, _: &Id, _: bool) -> Result<Organization> {
        Err(empty())
    }
    async fn delete(&self, _: &Id) -> Result<()> {
        Err(empty())
    }
}

#[async_trait]
impl IamMembershipRepository for NoDirectory {
    async fn upsert(&self, _: IamMembership, _: &[Id], _: &Id) -> Result<()> {
        Err(empty())
    }
    async fn list_for_user(&self, _: &Id) -> Result<Vec<IamMembership>> {
        Ok(Vec::new())
    }
    async fn list_for_org(&self, _: &Id) -> Result<Vec<IamMembership>> {
        Ok(Vec::new())
    }
    async fn assigned_roles(&self, _: &Id, _: &Id) -> Result<Vec<IamAssignedRole>> {
        Ok(Vec::new())
    }
    async fn role_id_for_purpose(&self, _: &Id, _: &str) -> Result<Id> {
        Err(empty())
    }
    async fn remove(&self, _: &Id, _: &Id) -> Result<()> {
        Err(empty())
    }
}
