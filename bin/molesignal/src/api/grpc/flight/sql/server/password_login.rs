// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Flight SQL 的账号密码握手：和 Web 登录共用输错次数，开了多因素认证的账号和有登录要求的组织
//! 拒绝。

use std::{net::IpAddr, sync::Arc};

use tonic::{Request, Status};

use super::{FlightSqlGrpc, error_to_status};
use crate::{
    api::http::client_ip::ClientIpResolverHandle,
    app::iam::signin_limit::{SigninAccount, SigninError, SigninLimiter},
    domain::iam::User,
    infra::mfa::{MfaFactorRepository, SigninPolicyRepository},
    shared::ids::Id,
};

impl FlightSqlGrpc {
    /// 拒绝开了多因素认证的账号用账号密码握手：密码之外没有地方输验证码，只能用 API token。
    pub fn with_second_factors(mut self, second_factors: Arc<dyn MfaFactorRepository>) -> Self {
        self.second_factors = Some(second_factors);
        self
    }

    pub fn with_signin_policies(mut self, policies: Arc<dyn SigninPolicyRepository>) -> Self {
        self.signin_policies = Some(policies);
        self
    }

    pub(crate) fn with_signin_limiter(
        mut self,
        limiter: Arc<SigninLimiter>,
        client_ip: ClientIpResolverHandle,
    ) -> Self {
        self.signin_limiter = Some((limiter, client_ip));
        self
    }

    /// 核对账号密码，和 Web 登录共用输错次数。输错太多次时不再核对。
    pub(super) async fn authenticate_password(
        &self,
        email: &str,
        password: &str,
        source: Option<IpAddr>,
    ) -> Result<User, Status> {
        let attempt = match &self.signin_limiter {
            Some((limiter, _)) => Some(
                limiter
                    .begin(&SigninAccount::Email(email), source)
                    .await
                    .map_err(signin_error_to_status)?,
            ),
            None => None,
        };

        let outcome = self.iam.authenticate(email, password).await;

        if let Some(attempt) = attempt {
            attempt.settle(&outcome).await;
        }

        outcome.map_err(|e| Status::unauthenticated(e.to_string()))
    }

    /// 这个请求的来源，规则和 HTTP 一样：代理把转发请求头放在 metadata 里。
    pub(super) fn signin_source<T>(&self, request: &Request<T>) -> Option<IpAddr> {
        let (_, client_ip) = self.signin_limiter.as_ref()?;

        client_ip.signin_source(
            &request.metadata().clone().into_headers(),
            request.remote_addr(),
        )
    }

    /// 组织有登录要求时拒绝账号密码：密码握手过不了多因素认证，也不是单点登录。root 也一样，
    /// 它可以用 API token。
    pub(super) async fn refuse_required_signin(&self, org_id: &Id) -> Result<(), Status> {
        let Some(policies) = &self.signin_policies else {
            return Ok(());
        };

        let policy = policies.get(org_id).await.map_err(error_to_status)?;

        if policy.require_mfa || policy.sso_only {
            return Err(Status::unauthenticated(
                "this organization has sign-in requirements: connect with an API token (mspat-...) as the password",
            ));
        }

        Ok(())
    }

    /// 账号开了多因素认证时拒绝密码握手。
    pub(super) async fn refuse_second_factor_accounts(&self, user_id: &Id) -> Result<(), Status> {
        let Some(second_factors) = &self.second_factors else {
            return Ok(());
        };

        let enabled = second_factors
            .enabled_among(std::slice::from_ref(user_id))
            .await
            .map_err(error_to_status)?;

        if enabled.contains(user_id) {
            return Err(Status::unauthenticated(
                "this account uses multi-factor authentication: connect with an API token (mspat-...) as the password",
            ));
        }

        Ok(())
    }
}

fn signin_error_to_status(error: SigninError) -> Status {
    match error {
        SigninError::Refused(refused) => Status::resource_exhausted(refused.message()),
        SigninError::Failed(error) => error_to_status(error),
    }
}
