// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! GitHub 与 GitHub Enterprise Server 的身份读取。
//!
//! GitHub 只有 OAuth 2.0：主体是不会变的数字 `id`，邮箱取已验证的主邮箱
//! （`user:email`），组织写作 `my-org`、团队写作 `my-org/sre`（`read:org`）。

use serde_json::Value;

use super::OAuthClient;
use crate::{
    infra::sso::claims::ExternalIdentity,
    shared::{Error, Result},
};

/// `userinfo_url` 是 `{API 根地址}/user`：github.com 是 `https://api.github.com/user`，
/// Enterprise Server 是 `https://{host}/api/v3/user`。
pub(super) async fn identity(
    client: &OAuthClient,
    userinfo_url: &str,
    access_token: &str,
) -> Result<ExternalIdentity> {
    let api = userinfo_url
        .trim_end_matches('/')
        .strip_suffix("/user")
        .ok_or_else(|| Error::invalid("the GitHub user endpoint must end with /user"))?;

    let emails_url = format!("{api}/user/emails?per_page=100");
    let orgs_url = format!("{api}/user/orgs?per_page=100");
    let teams_url = format!("{api}/user/teams?per_page=100");

    let (user, emails, orgs, teams) = tokio::join!(
        client.get_json(userinfo_url, access_token),
        client.get_json(&emails_url, access_token),
        client.get_json(&orgs_url, access_token),
        client.get_json(&teams_url, access_token),
    );

    let user = user?;

    let subject = match user.get("id") {
        Some(Value::Number(id)) => id.to_string(),
        _ => return Err(Error::unauthorized("GitHub user has no id")),
    };

    let email = primary_verified_email(&emails?).ok_or_else(|| {
        Error::unauthorized(
            "the GitHub account has no verified email address, or the user:email scope was not granted",
        )
    })?;

    let name = ["name", "login"].into_iter().find_map(|field| {
        user.get(field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    });

    Ok(ExternalIdentity {
        subject,
        email,
        name,
        groups: group_names(listed(orgs, "organizations"), listed(teams, "teams")),
        email_verified: Some(true),
    })
}

/// 组织和团队只用来映射角色和限制登录：读不到时按没有组处理并记一条日志，
/// 不让整次登录失败。
fn listed(result: Result<Value>, what: &str) -> Value {
    result.unwrap_or_else(|error| {
        tracing::warn!(%error, "could not read the GitHub user's {what}; treating as none");
        Value::Array(Vec::new())
    })
}

fn primary_verified_email(emails: &Value) -> Option<String> {
    let emails = emails.as_array()?;
    let flag = |email: &Value, name: &str| email.get(name).and_then(Value::as_bool) == Some(true);
    let verified = emails.iter().filter(|email| flag(email, "verified"));

    verified
        .clone()
        .find(|email| flag(email, "primary"))
        .or_else(|| verified.clone().next())
        .and_then(|email| email.get("email"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn group_names(orgs: Value, teams: Value) -> Vec<String> {
    let text = |value: &Value, field: &str| {
        value
            .get(field)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };

    let orgs = orgs
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|org| text(org, "login"));

    let teams = teams.as_array().into_iter().flatten().filter_map(|team| {
        let org = team
            .get("organization")
            .and_then(|org| text(org, "login"))?;

        let slug = text(team, "slug")?;
        Some(format!("{org}/{slug}"))
    });

    let mut groups = orgs.chain(teams).collect::<Vec<_>>();
    groups.sort();
    groups.dedup();
    groups
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn primary_verified_email_wins_over_other_verified_addresses() {
        let emails = json!([
            { "email": "old@example.com", "primary": false, "verified": true },
            { "email": "main@example.com", "primary": true, "verified": true },
            { "email": "new@example.com", "primary": false, "verified": false }
        ]);
        assert_eq!(
            primary_verified_email(&emails).as_deref(),
            Some("main@example.com")
        );
    }

    #[test]
    fn an_unverified_primary_falls_back_to_a_verified_address() {
        let emails = json!([
            { "email": "main@example.com", "primary": true, "verified": false },
            { "email": "work@example.com", "primary": false, "verified": true }
        ]);
        assert_eq!(
            primary_verified_email(&emails).as_deref(),
            Some("work@example.com")
        );
        assert_eq!(primary_verified_email(&json!([])), None);
    }

    #[test]
    fn organizations_and_teams_become_groups() {
        let orgs = json!([{ "login": "acme" }, { "login": "tools" }]);
        let teams = json!([
            { "slug": "sre", "organization": { "login": "acme" } },
            { "slug": "", "organization": { "login": "acme" } }
        ]);
        assert_eq!(group_names(orgs, teams), ["acme", "acme/sre", "tools"]);
    }
}
