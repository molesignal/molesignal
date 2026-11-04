// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! `/rum/issues`: the errors of an application as issues people can act on.
//!
//! An issue is what the errors that are alike are one thing of. What it is and what is being done
//! about it live in Postgres (`rum_issues`, kept as errors are written); how many sessions and
//! people it touched in a range and how it went over the range are counted from the errors
//! themselves. A list joins the two; the reads are the app layer's, which the tools of Mole Agent
//! use too.
//!
//! - `GET /rum/issues` lists the issues with errors in a range of an application, by state, holder
//!   and words, with their numbers and a trend each.
//! - `GET /rum/issues/{id}` is one issue with its numbers in the range, its latest errors and the
//!   stack of the latest that has one.
//! - `PATCH /rum/issues/{id}` sets its state, its team and the person who has it.

use std::collections::HashSet;

use axum::{
    Extension, Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Deserializer};

use super::{address::json_list, reader::Reader};
use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{
        iam::IamContext,
        rum::issues::{
            self, DEFAULT_LIMIT, Detail, Listing, checked_id, descending, present,
            select::{Holder, Sort, StatusFilter},
        },
    },
    domain::{
        iam::permission,
        rum::{RumIssue, RumIssueChange, RumIssueStatus},
    },
    infra::rum::query::Filter,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

#[derive(Debug, Default, Deserialize)]
pub(super) struct ListParams {
    application: Option<String>,
    env: Option<String>,
    /// What sent the errors: `browser`, `flutter`…
    source: Option<String>,
    /// More conditions the errors counted have to meet, as the explorer says them: a JSON list.
    #[serde(default, deserialize_with = "json_list")]
    filters: Vec<Filter>,
    from: Option<i64>,
    to: Option<i64>,
    /// `unresolved` (the default), `all`, or one state.
    status: Option<String>,
    /// A user id, or `none`.
    assignee: Option<String>,
    /// A team id, or `none`.
    team: Option<String>,
    /// Words that the type, the message or the place of an issue has.
    search: Option<String>,
    /// Only the issues that are new in the range or went back in it.
    fresh: Option<bool>,
    /// `sessions` (the default), `users`, `occurrences`, `last_seen`, `first_seen` or `status`.
    sort: Option<String>,
    /// `asc` or `desc`; the biggest or the latest first when not given.
    order: Option<String>,
    limit: Option<usize>,
}

#[permission(any("rum.issues.read", "sys.telemetry.read"))]
pub(super) async fn list(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Query(params): Query<ListParams>,
) -> Result<Json<Listing>> {
    let application = present(params.application.as_deref())
        .ok_or_else(|| Error::invalid("application is required"))?;

    let sort = Sort::parse(params.sort.as_deref())?;

    let query = issues::Query {
        application: application.to_string(),
        env: params.env,
        source: params.source,
        filters: params.filters,
        from: params.from,
        to: params.to,
        status: StatusFilter::parse(params.status.as_deref())?,
        assignee: Holder::parse(params.assignee.as_deref()),
        team: Holder::parse(params.team.as_deref()),
        search: params.search.unwrap_or_default(),
        fresh: params.fresh.unwrap_or_default(),
        sort,
        descending: descending(params.order.as_deref(), sort)?,
        limit: params.limit.unwrap_or(DEFAULT_LIMIT),
    };

    let reader = Reader::new(&state, &iam);

    issues::list(&reader, state.telemetry.rum_issues.as_ref(), &query)
        .await
        .map(Json)
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct DetailParams {
    from: Option<i64>,
    to: Option<i64>,
}

#[permission(any("rum.issues.read", "sys.telemetry.read"))]
pub(super) async fn get(
    State(state): State<AppState>,
    Extension(iam): Extension<IamContext>,
    Path(id): Path<String>,
    Query(params): Query<DetailParams>,
) -> Result<Json<Detail>> {
    let reader = Reader::new(&state, &iam);

    issues::detail(
        &reader,
        state.telemetry.rum_issues.as_ref(),
        &id,
        params.from,
        params.to,
    )
    .await
    .map(Json)
}

/// What a change says. A team or a person left out stays; given as `null` or an empty text, it is
/// taken off.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct UpdateBody {
    status: Option<RumIssueStatus>,
    #[serde(default, deserialize_with = "named_or_cleared")]
    assignee_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "named_or_cleared")]
    team_id: Option<Option<String>>,
}

/// A field that is there, even as `null`, is a change; one that is not there is not.
fn named_or_cleared<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Option<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

/// An id that was sent, as opposed to an empty one.
fn named(value: Option<String>) -> Option<Id> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(Id::from_string)
}

#[permission("rum.issues.manage")]
pub(super) async fn update(
    State(state): State<AppState>,
    Extension(ctx): Extension<IamContext>,
    Path(id): Path<String>,
    Json(body): Json<UpdateBody>,
) -> Result<Json<RumIssue>> {
    let id = checked_id(&id)?;

    let change = RumIssueChange {
        status: body.status,
        assignee: body.assignee_id.map(named),
        team: body.team_id.map(named),
    };

    change.validate()?;

    if let Some(Some(user)) = &change.assignee {
        ensure_member(&state, &ctx.org_id, user).await?;
    }

    if let Some(Some(team)) = &change.team {
        ensure_team(&state, &ctx.org_id, team).await?;
    }

    let updated = state
        .telemetry
        .rum_issues
        .update(
            &ctx.org_id,
            id,
            &change,
            ctx.principal_id(),
            TimestampMicros::now(),
        )
        .await?
        .ok_or_else(|| Error::not_found("RUM issue"))?;

    activity_audit::record(
        &state,
        &ctx,
        "rum.issue.update",
        "rum_issue",
        &updated.id,
        serde_json::json!({
            "applicationId": updated.application_id,
            "status": updated.status,
            "assigneeId": updated.assignee_id,
            "teamId": updated.team_id,
        }),
    )
    .await;

    Ok(Json(updated))
}

async fn ensure_team(state: &AppState, org_id: &Id, team_id: &Id) -> Result<()> {
    // A team that does not exist is a wrong argument of this request, not a missing issue.
    let team = match state.iam.teams.get(team_id).await {
        Err(Error::NotFound(_)) => {
            return Err(Error::invalid(
                "teamId must reference a team in this organization",
            ));
        }
        other => other?,
    };

    if &team.org_id != org_id {
        return Err(Error::invalid(
            "teamId must reference a team in this organization",
        ));
    }

    Ok(())
}

async fn ensure_member(state: &AppState, org_id: &Id, user_id: &Id) -> Result<()> {
    let members = state
        .iam
        .service
        .iam_memberships
        .list_for_org(org_id)
        .await?
        .into_iter()
        .map(|membership| membership.user_id.0)
        .collect::<HashSet<_>>();

    if !members.contains(user_id.as_str()) {
        return Err(Error::invalid(
            "the user must be a member of this organization",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_change_tells_what_it_leaves_out_from_what_it_takes_off() {
        let left_out: UpdateBody = serde_json::from_value(json!({ "status": "resolved" })).unwrap();

        assert_eq!(left_out.status, Some(RumIssueStatus::Resolved));
        assert_eq!(left_out.assignee_id, None);
        assert_eq!(left_out.team_id, None);

        let taken_off: UpdateBody =
            serde_json::from_value(json!({ "assigneeId": null, "teamId": "web" })).unwrap();

        assert_eq!(taken_off.assignee_id, Some(None));
        assert_eq!(taken_off.team_id, Some(Some("web".into())));
        assert_eq!(taken_off.status, None);
    }

    #[test]
    fn an_empty_id_takes_a_person_off() {
        assert_eq!(named(Some("  ".into())), None);
        assert_eq!(named(Some(" ada ".into())), Some(Id::from_string("ada")));
        assert_eq!(named(None), None);
    }
}
