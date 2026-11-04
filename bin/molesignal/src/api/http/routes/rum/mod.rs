// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Real user monitoring: the events SDKs send, the applications that send them, and the reads the
//! pages are made of.
//!
//! - `POST /rum/events` takes events in the shape of the contract in `contracts/rum/events/`;
//!   `POST /rum/replay` takes the segments of a session replay.
//! - `/rum/applications` creates and changes the applications; an event is written to the
//!   application its client token is bound to. `GET /rum/config` is what the settings of the
//!   application decide for an SDK that asks at start.
//! - `/rum/search`, `/rum/facets`, `/rum/histogram`, `/rum/aggregate` and `/rum/schema/{type}` are
//!   the explorer: a list of rows of one type, the commonest values of a field, the bars over
//!   time, numbers per group, the columns. A type is `sessions`, `views`, `actions`, `errors`,
//!   `resources`, `long_tasks` or `vitals`.
//! - `/rum/analytics/funnel`, `/rum/analytics/paths` and `/rum/analytics/retention` are product
//!   analytics over sessions: who did a series of steps in order, where they went from a page, and
//!   who came back.
//! - `/rum/playlists` keeps lists of sessions for a team to go through one after the other, each
//!   session with the moment worth opening it at.
//! - `POST /rum/alerts/preview` reads the figure an alert rule on RUM would watch over the last
//!   stretch of time, by the code that evaluates the rule.
//! - `/rum/issues` lists the errors of an application as issues people can act on, `/rum/issues/{id}`
//!   is one with its latest errors and its stack, and a `PATCH` sets its state, team and holder.
//! - `/rum/{type}/{id}` is one row, `/rum/sessions/{id}/timeline` everything a session did, and
//!   `/rum/sessions/{id}/related-traces` with `/rum/traces/{trace_id}/session` join sessions and
//!   backend traces.
//! - `/rum/sessions/{id}/comments` are what people say about moments of a replay, and
//!   `/rum/sessions/{id}/summary` what a model makes of the session.
//! - `/rum/keys` lists the keys of a JSON column (the feature flags, the custom timings),
//!   `/rum/distribution` how the values of a number fall, and
//!   `/rum/heatmaps/{clicks,scroll,snapshot}` are where people click and how far down they go on a
//!   page, and a recorded page to draw that over.
//! - `POST /rum/panel-statement` is the statement a dashboard panel runs to draw what the
//!   explorer draws.
//!
//! The rows are in ordinary Logs streams (`domain::rum::event`) and are read with SQL, so
//! what the query engine masks or prunes holds here too.

use axum::{
    Extension, Router,
    extract::DefaultBodyLimit,
    routing::{delete, get, patch, post},
};

use crate::{
    api::AppState,
    domain::rum::event::RumStream,
    infra::rum::{intake::MAX_DECODED_BYTES, replay::MAX_REPLAY_SEGMENT_BYTES},
};

mod address;
mod aggregate;
mod alerts;
mod analytics;
mod applications;
mod comments;
mod distribution;
mod entity;
mod events;
mod facets;
mod heatmaps;
mod histogram;
mod issues;
mod keys;
mod panel;
mod playlists;
mod reader;
mod replay;
mod schema;
mod search;
mod selection;
mod summary;
mod symbolication;
mod timeline;
mod traces;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/rum/events",
            post(events::receive).layer(DefaultBodyLimit::max(MAX_DECODED_BYTES)),
        )
        .route(
            "/rum/replay",
            post(replay::receive).layer(DefaultBodyLimit::max(MAX_REPLAY_SEGMENT_BYTES as usize)),
        )
        .route("/rum/replay/{sessionId}", get(replay::read))
        .route("/rum/config", get(applications::sdk_config))
        .route(
            "/rum/applications",
            get(applications::list).post(applications::create),
        )
        .route(
            "/rum/applications/{id}",
            get(applications::get)
                .patch(applications::update)
                .delete(applications::delete),
        )
        .route(
            "/rum/search",
            get(search::search_get).post(search::search_post),
        )
        .route(
            "/rum/facets",
            get(facets::facets_get).post(facets::facets_post),
        )
        .route(
            "/rum/histogram",
            get(histogram::histogram_get).post(histogram::histogram_post),
        )
        .route(
            "/rum/aggregate",
            get(aggregate::aggregate_get).post(aggregate::aggregate_post),
        )
        .route(
            "/rum/analytics/funnel",
            get(analytics::funnel_get).post(analytics::funnel_post),
        )
        .route(
            "/rum/analytics/paths",
            get(analytics::paths_get).post(analytics::paths_post),
        )
        .route(
            "/rum/analytics/retention",
            get(analytics::retention_get).post(analytics::retention_post),
        )
        .route(
            "/rum/playlists",
            get(playlists::list).post(playlists::create),
        )
        .route(
            "/rum/playlists/{id}",
            get(playlists::get)
                .patch(playlists::update)
                .delete(playlists::delete),
        )
        .route(
            "/rum/playlists/{id}/sessions",
            post(playlists::add_sessions),
        )
        .route(
            "/rum/playlists/{id}/sessions/{sessionId}",
            delete(playlists::remove_session),
        )
        .route("/rum/alerts/preview", post(alerts::preview))
        .route("/rum/issues", get(issues::list))
        .route("/rum/issues/{id}", get(issues::get).patch(issues::update))
        .route("/rum/schema/{kind}", get(schema::schema))
        .route("/rum/sessions/{id}/timeline", get(timeline::timeline))
        .route("/rum/sessions/{id}/playlists", get(playlists::of_session))
        .route(
            "/rum/sessions/{id}/comments",
            get(comments::list).post(comments::create),
        )
        .route(
            "/rum/sessions/{id}/comments/{commentId}",
            patch(comments::update).delete(comments::delete),
        )
        .route(
            "/rum/sessions/{id}/summary",
            get(summary::read).post(summary::generate),
        )
        .route("/rum/keys", get(keys::keys_get).post(keys::keys_post))
        .route(
            "/rum/distribution",
            get(distribution::distribution_get).post(distribution::distribution_post),
        )
        .route("/rum/panel-statement", post(panel::statement))
        .route(
            "/rum/heatmaps/clicks",
            get(heatmaps::clicks_get).post(heatmaps::clicks_post),
        )
        .route(
            "/rum/heatmaps/scroll",
            get(heatmaps::scroll_get).post(heatmaps::scroll_post),
        )
        .route(
            "/rum/heatmaps/snapshot",
            get(heatmaps::snapshot_get).post(heatmaps::snapshot_post),
        )
        .route(
            "/rum/sessions/{id}/related-traces",
            get(traces::related_traces),
        )
        .route("/rum/traces/{traceId}/session", get(traces::trace_session))
        .merge(entity_routes())
}

/// One route per type, each telling the handler which stream it is for.
fn entity_routes() -> Router<AppState> {
    RumStream::ALL
        .into_iter()
        .fold(Router::new(), |router, stream| {
            router.route(
                &format!("/rum/{}/{{id}}", stream.collection()),
                get(entity::entity).layer(Extension(stream)),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_routes_do_not_conflict() {
        let _ = routes();
    }
}
