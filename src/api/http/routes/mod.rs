// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use axum::Router;

use crate::api::AppState;

pub mod activity_audit;
pub mod alerting;
pub mod annotations;
pub mod api_tokens;
pub mod apm;
pub mod audit;
pub mod auth;
pub mod billing;
pub mod cipher_keys;
pub mod clusters;
pub mod connectors;
pub mod dashboards;
pub mod debug_artifacts;
pub mod email_domains;
pub mod extend_tables;
pub mod field_masking;
pub mod files;
pub mod folders;
pub mod functions;
pub mod health;
pub mod home;
pub mod iam;
pub mod incident_groups;
pub mod ingest;
pub mod instance;
pub mod invitations;
pub mod jwt_secrets;
pub mod license;
pub mod log_patterns;
pub mod me;
pub mod metrics;
pub mod model_prices;
pub mod mutes;
pub mod node;
pub mod notify;
pub mod onboarding;
pub mod profiles;
pub mod profiling;
pub mod query;
pub mod regex_patterns;
pub mod reports;
pub mod resource_shares;
pub mod roles;
pub mod rum;
pub mod saved_views;
pub mod scheduled_pipelines;
pub mod schedules;
pub mod search_jobs;
pub mod semantic_groups;
pub mod sso;
pub mod streams;
pub mod system;
pub mod teams;
pub mod traces;
pub mod web;

// 付费版路由集合。
pub mod domains;
pub mod intelligence;
pub mod marketplace;

pub fn api_v1(state: AppState) -> Router<AppState> {
    let r = Router::new()
        .merge(health::routes())
        .merge(home::routes())
        .merge(auth::routes())
        .merge(sso::routes())
        .merge(iam::routes())
        .merge(teams::routes())
        .merge(invitations::routes())
        .merge(email_domains::routes())
        .merge(billing::routes())
        .merge(ingest::routes())
        .merge(onboarding::routes())
        .merge(query::routes())
        .merge(dashboards::routes())
        .merge(folders::routes())
        .merge(resource_shares::routes())
        .merge(alerting::routes())
        .merge(notify::routes())
        .merge(schedules::routes())
        .merge(mutes::routes())
        .merge(system::routes())
        .merge(traces::routes())
        .merge(apm::routes())
        .merge(rum::routes())
        .merge(clusters::routes())
        .merge(cipher_keys::routes())
        .merge(connectors::routes())
        .merge(extend_tables::routes())
        .merge(scheduled_pipelines::routes())
        .merge(annotations::routes())
        .merge(incident_groups::routes())
        .merge(semantic_groups::routes())
        .merge(search_jobs::routes())
        .merge(functions::routes())
        .merge(debug_artifacts::routes())
        .merge(log_patterns::routes())
        .merge(saved_views::routes())
        .merge(reports::routes())
        .merge(streams::routes())
        .merge(metrics::api_routes())
        .merge(me::routes())
        .merge(files::routes())
        .merge(field_masking::routes())
        .merge(profiles::routes())
        .merge(profiling::routes())
        .merge(api_tokens::routes())
        .merge(instance::routes())
        .merge(node::routes())
        .merge(audit::routes())
        .merge(jwt_secrets::routes())
        .merge(license::routes())
        .merge(model_prices::routes())
        .merge(regex_patterns::routes())
        .merge(roles::routes())
        .merge(web::routes());

    let r = r
        .merge(intelligence::routes())
        .merge(marketplace::routes())
        .merge(domains::routes());

    r.with_state(state)
}
