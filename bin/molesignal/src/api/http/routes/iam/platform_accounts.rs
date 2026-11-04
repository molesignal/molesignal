// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Looking accounts up from the platform organization.
//!
//! The platform organization has no members, so platform permissions are granted to accounts
//! wherever they belong. Whoever may read those grants can find an account by its email or name,
//! and read back the accounts a list of grants names.

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::AppState,
    app::iam::IamContext,
    domain::iam::{User, permission},
    shared::{Error, Result, ids::Id},
};

/// How many accounts one search answers with: enough to pick from, and a reason to type more.
const SEARCH_LIMIT: usize = 20;

/// How many accounts can be named at once: more than a page of grants shows.
const IDS_LIMIT: usize = 200;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/iam/platform/accounts", get(accounts))
}

#[derive(Debug, Default, Deserialize)]
struct AccountsQuery {
    /// Part of an email or a name.
    #[serde(default)]
    q: Option<String>,
    /// Account ids, separated by commas. Takes the place of `q`.
    #[serde(default)]
    ids: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountView {
    id: String,
    email: String,
    display_name: String,
    disabled: bool,
}

impl From<User> for AccountView {
    fn from(user: User) -> Self {
        Self {
            id: user.id.0,
            email: user.email,
            display_name: user.display_name,
            disabled: user.disabled,
        }
    }
}

#[permission("sys.administrators.read")]
async fn accounts(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Query(query): Query<AccountsQuery>,
) -> Result<Json<Vec<AccountView>>> {
    if let Some(ids) = query.ids.as_deref() {
        let ids = named_ids(ids)?;
        let users = state.iam.service.users.get_many(&ids).await?;

        return Ok(Json(users.into_iter().map(Into::into).collect()));
    }

    let needle = query.q.as_deref().unwrap_or_default();

    if needle.trim().is_empty() {
        return Ok(Json(Vec::new()));
    }

    let users = state.iam.service.users.list().await?;

    Ok(Json(
        matching(users, needle)
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

fn named_ids(ids: &str) -> Result<Vec<Id>> {
    let ids = ids
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(Id::from_string)
        .collect::<Vec<_>>();

    if ids.len() > IDS_LIMIT {
        return Err(Error::invalid(format!(
            "at most {IDS_LIMIT} accounts can be named at once"
        )));
    }

    Ok(ids)
}

/// Accounts whose email or name holds the text, whatever its case: an exact email first, then by
/// email, up to [`SEARCH_LIMIT`].
fn matching(users: Vec<User>, needle: &str) -> Vec<User> {
    let needle = needle.trim().to_lowercase();

    let mut found = users
        .into_iter()
        .filter(|user| {
            user.email.to_lowercase().contains(&needle)
                || user.display_name.to_lowercase().contains(&needle)
        })
        .collect::<Vec<_>>();

    found.sort_by_key(|user| {
        let email = user.email.to_lowercase();
        (email != needle, email)
    });

    found.truncate(SEARCH_LIMIT);
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::iam::UserStatus, shared::time::TimestampMicros};

    fn account(email: &str, display_name: &str) -> User {
        User {
            id: Id::from_string(email),
            email: email.into(),
            display_name: display_name.into(),
            avatar_url: None,
            bio: String::new(),
            password_hash: String::new(),
            disabled: false,
            status: UserStatus::default(),
            created_at: TimestampMicros(0),
            sessions_valid_after: TimestampMicros(0),
        }
    }

    fn emails(users: Vec<User>) -> Vec<String> {
        users.into_iter().map(|user| user.email).collect()
    }

    #[test]
    fn an_account_is_found_by_part_of_its_email_or_name_in_any_case() {
        let users = vec![
            account("ada@example.com", "Ada"),
            account("grace@example.com", "Grace Hopper"),
            account("linus@other.org", "Linus"),
        ];

        assert_eq!(
            emails(matching(users.clone(), " EXAMPLE ")),
            ["ada@example.com", "grace@example.com"]
        );
        assert_eq!(emails(matching(users, "hopper")), ["grace@example.com"]);
    }

    #[test]
    fn the_exact_email_comes_first() {
        let users = vec![
            account("a.ada@example.com", "A"),
            account("ada@example.com", "B"),
        ];

        assert_eq!(
            emails(matching(users, "ada@example.com")),
            ["ada@example.com", "a.ada@example.com"]
        );
    }

    #[test]
    fn a_search_answers_with_a_limited_number_of_accounts() {
        let users = (0..SEARCH_LIMIT + 5)
            .map(|index| account(&format!("user{index}@example.com"), "User"))
            .collect();

        assert_eq!(matching(users, "example").len(), SEARCH_LIMIT);
    }

    #[test]
    fn a_list_of_ids_is_bounded_and_ignores_blanks() {
        let named = named_ids("a, b,,c ").expect("three ids");

        assert_eq!(
            named,
            [
                Id::from_string("a"),
                Id::from_string("b"),
                Id::from_string("c")
            ]
        );

        let too_many = vec!["x"; IDS_LIMIT + 1].join(",");

        assert!(named_ids(&too_many).is_err());
    }
}
