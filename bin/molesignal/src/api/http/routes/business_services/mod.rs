// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Business services: the internal view of what customers rely on, the
//! dependencies each one has, its health and the status-page components that
//! stand for it.

use std::collections::HashMap;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{AppState, http::routes::activity_audit},
    app::{business_service::BusinessServiceInput, iam::IamContext},
    domain::{
        business_service::{BusinessService, BusinessServiceDependency},
        iam::permission,
        status_page::{ComponentStatus, ServiceLinkMode, StatusPageComponent},
    },
    shared::{Result, ids::Id},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/business-services", get(list).post(create))
        .route(
            "/business-services/{id}",
            get(get_one).put(update).delete(remove),
        )
}

#[derive(Debug, Deserialize)]
struct WriteRequest {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    dependencies: Vec<BusinessServiceDependency>,
}

impl From<WriteRequest> for BusinessServiceInput {
    fn from(request: WriteRequest) -> Self {
        Self {
            name: request.name,
            description: request.description,
            dependencies: request.dependencies,
        }
    }
}

/// A status-page component that stands for the service.
#[derive(Debug, Serialize)]
struct LinkedComponent {
    status_page_id: Id,
    status_page_name: String,
    component_id: Id,
    component_name: String,
    component_status: ComponentStatus,
    mode: ServiceLinkMode,
    notify_subscribers: bool,
}

#[derive(Debug, Serialize)]
struct BusinessServiceView {
    #[serde(flatten)]
    service: BusinessService,
    linked_components: Vec<LinkedComponent>,
}

#[permission("business_services.read")]
async fn list(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
) -> Result<Json<Vec<BusinessServiceView>>> {
    let services = state.business_services.list(&context.org_id).await?;
    let mut linked = linked_components(&state, &context.org_id, None).await?;

    Ok(Json(
        services
            .into_iter()
            .map(|service| BusinessServiceView {
                linked_components: linked.remove(&service.id).unwrap_or_default(),
                service,
            })
            .collect(),
    ))
}

#[permission("business_services.read")]
async fn get_one(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<Json<BusinessServiceView>> {
    let id = Id(id);
    let service = state.business_services.get(&context.org_id, &id).await?;
    let mut linked = linked_components(&state, &context.org_id, Some(&id)).await?;

    Ok(Json(BusinessServiceView {
        linked_components: linked.remove(&id).unwrap_or_default(),
        service,
    }))
}

#[permission("business_services.manage")]
async fn create(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Json(request): Json<WriteRequest>,
) -> Result<(StatusCode, Json<BusinessServiceView>)> {
    let service = state
        .business_services
        .create(&context.org_id, &context.user_id, request.into())
        .await?;

    record_audit(&state, &context, &service, "create").await;

    Ok((
        StatusCode::CREATED,
        Json(BusinessServiceView {
            service,
            linked_components: Vec::new(),
        }),
    ))
}

#[permission("business_services.manage")]
async fn update(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
    Json(request): Json<WriteRequest>,
) -> Result<Json<BusinessServiceView>> {
    let id = Id(id);

    let service = state
        .business_services
        .update(&context.org_id, &id, request.into())
        .await?;

    record_audit(&state, &context, &service, "update").await;
    let mut linked = linked_components(&state, &context.org_id, Some(&id)).await?;

    Ok(Json(BusinessServiceView {
        linked_components: linked.remove(&id).unwrap_or_default(),
        service,
    }))
}

#[permission("business_services.manage")]
async fn remove(
    State(state): State<AppState>,
    Extension(context): Extension<IamContext>,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let id = Id(id);
    let service = state.business_services.get(&context.org_id, &id).await?;
    state.business_services.delete(&context.org_id, &id).await?;
    record_audit(&state, &context, &service, "delete").await;
    Ok(StatusCode::NO_CONTENT)
}

/// The components that stand for business services, by service, with the
/// names of their pages.
async fn linked_components(
    state: &AppState,
    org_id: &Id,
    business_service_id: Option<&Id>,
) -> Result<HashMap<Id, Vec<LinkedComponent>>> {
    let components = state
        .business_services
        .linked_components(org_id, business_service_id)
        .await?;

    if components.is_empty() {
        return Ok(HashMap::new());
    }

    let page_names: HashMap<Id, String> = state
        .status_pages
        .list_pages(org_id, None)
        .await?
        .into_iter()
        .map(|page| (page.id, page.name))
        .collect();

    let mut linked: HashMap<Id, Vec<LinkedComponent>> = HashMap::new();

    for component in components {
        let Some(link) = component.business_service.clone() else {
            continue;
        };

        linked
            .entry(link.business_service_id)
            .or_default()
            .push(linked_component(
                component,
                &page_names,
                link.mode,
                link.notify_subscribers,
            ));
    }

    Ok(linked)
}

fn linked_component(
    component: StatusPageComponent,
    page_names: &HashMap<Id, String>,
    mode: ServiceLinkMode,
    notify_subscribers: bool,
) -> LinkedComponent {
    LinkedComponent {
        status_page_name: page_names
            .get(&component.status_page_id)
            .cloned()
            .unwrap_or_default(),
        status_page_id: component.status_page_id,
        component_id: component.id,
        component_name: component.name,
        component_status: component.status,
        mode,
        notify_subscribers,
    }
}

async fn record_audit(
    state: &AppState,
    context: &IamContext,
    service: &BusinessService,
    verb: &str,
) {
    activity_audit::record(
        state,
        context,
        &format!("business_service.{verb}"),
        "business_service",
        service.id.as_str(),
        serde_json::json!({
            "name": service.name,
            "dependency_count": service.dependencies.len(),
        }),
    )
    .await;
}
