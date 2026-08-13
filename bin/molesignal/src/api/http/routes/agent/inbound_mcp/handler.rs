// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::{borrow::Cow, collections::HashMap, sync::Arc};

use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::*,
    service::{RequestContext, SubscriptionContext},
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::{
    prompts, resources,
    runtime::{CatalogEvent, InboundMcpAdapterRuntime},
    tasks, tools,
};
use crate::{
    agent::inbound_mcp::InboundMcpSettings,
    api::{AppState, http::middleware::auth::AuthenticatedCredential},
    app::iam::IamContext,
    shared::Error,
};

#[derive(Clone)]
pub(super) struct InboundMcpHandler {
    pub(super) state: AppState,
    pub(super) runtime: Arc<InboundMcpAdapterRuntime>,
    session_principal: Arc<std::sync::Mutex<Option<String>>>,
    legacy_catalog_watcher: Arc<Mutex<Option<CancellationToken>>>,
    legacy_resource_subscriptions: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl InboundMcpHandler {
    pub fn new(state: AppState, runtime: Arc<InboundMcpAdapterRuntime>) -> Self {
        Self {
            state,
            runtime,
            session_principal: Arc::new(std::sync::Mutex::new(None)),
            legacy_catalog_watcher: Arc::new(Mutex::new(None)),
            legacy_resource_subscriptions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn start_legacy_catalog_watcher(
        &self,
        data: &InboundRequestData,
        peer: rmcp::service::Peer<RoleServer>,
    ) {
        let cancellation = CancellationToken::new();
        if let Some(previous) = self
            .legacy_catalog_watcher
            .lock()
            .await
            .replace(cancellation.clone())
        {
            previous.cancel();
        }
        let mut events = self.runtime.subscribe(&data.iam.org_id);
        crate::shared::trace_context::spawn_with_current_trace_context(async move {
            let mut refresh = tokio::time::interval(std::time::Duration::from_secs(30));
            refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            refresh.tick().await;
            loop {
                let notify_all = tokio::select! {
                    () = cancellation.cancelled() => break,
                    _ = refresh.tick() => {
                        if peer.is_transport_closed() {
                            break;
                        }
                        true
                    }
                    event = events.recv() => match event {
                        Ok(CatalogEvent::Tools) => {
                            let _ = peer.notify_tool_list_changed().await;
                            false
                        }
                        Ok(CatalogEvent::Prompts) => {
                            let _ = peer.notify_prompt_list_changed().await;
                            false
                        }
                        Ok(CatalogEvent::Resources) => {
                            let _ = peer.notify_resource_list_changed().await;
                            false
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => true,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                };
                if notify_all {
                    let _ = peer.notify_tool_list_changed().await;
                    let _ = peer.notify_prompt_list_changed().await;
                    let _ = peer.notify_resource_list_changed().await;
                }
            }
        });
    }
}

#[derive(Clone)]
pub(super) struct InboundRequestData {
    pub iam: IamContext,
    pub settings: InboundMcpSettings,
}

pub(super) fn request_data(
    handler: &InboundMcpHandler,
    context: &RequestContext<RoleServer>,
) -> Result<InboundRequestData, ErrorData> {
    let parts = context
        .extensions
        .get::<axum::http::request::Parts>()
        .ok_or_else(|| ErrorData::internal_error("missing authenticated HTTP context", None))?;
    let iam = parts
        .extensions
        .get::<IamContext>()
        .cloned()
        .ok_or_else(|| ErrorData::internal_error("missing authenticated identity", None))?;
    let settings = parts
        .extensions
        .get::<InboundMcpSettings>()
        .cloned()
        .ok_or_else(|| ErrorData::internal_error("missing Inbound MCP settings", None))?;
    let credential = parts
        .extensions
        .get::<AuthenticatedCredential>()
        .ok_or_else(|| ErrorData::internal_error("missing authenticated credential", None))?;
    let principal = credential.stable_key(&iam.org_id);
    let mut bound = handler
        .session_principal
        .lock()
        .map_err(|_| ErrorData::internal_error("MCP session identity lock failed", None))?;
    match bound.as_ref() {
        Some(existing) if existing != &principal => {
            return Err(ErrorData::invalid_request(
                "MCP session is bound to a different authenticated principal",
                None,
            ));
        }
        None => *bound = Some(principal),
        Some(_) => {}
    }
    Ok(InboundRequestData { iam, settings })
}

pub(super) fn protocol_error(error: Error) -> ErrorData {
    match error {
        Error::InvalidArgument(message) | Error::Validation { message, .. } => {
            ErrorData::invalid_params(message, None)
        }
        Error::NotFound(message) => ErrorData::resource_not_found(message, None),
        Error::Conflict(message) | Error::Unauthorized(message) | Error::Forbidden(message) => {
            ErrorData::invalid_request(message, None)
        }
        other => {
            tracing::warn!(error = %other, "Inbound MCP handler failed");
            ErrorData::internal_error("MoleSignal could not complete the request", None)
        }
    }
}

impl ServerHandler for InboundMcpHandler {
    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        let data = request_data(self, &context)?;
        context.peer.set_peer_info(request.clone());
        let mut info = self.get_info();
        if self
            .supported_protocol_versions()
            .contains(&request.protocol_version)
        {
            info.protocol_version = request.protocol_version;
        }
        if info.protocol_version < ProtocolVersion::V_2026_07_28 {
            self.start_legacy_catalog_watcher(&data, context.peer.clone())
                .await;
        }
        Ok(info)
    }

    async fn discover(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<DiscoverResult, ErrorData> {
        let _ = request_data(self, &context)?;
        Ok(DiscoverResult::from_server_info(
            self.supported_protocol_versions().into_owned(),
            self.get_info(),
        ))
    }

    fn supported_protocol_versions(&self) -> Cow<'static, [ProtocolVersion]> {
        Cow::Borrowed(&[
            ProtocolVersion::V_2026_07_28,
            ProtocolVersion::V_2025_11_25,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_03_26,
        ])
    }

    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tool_list_changed()
                .enable_resources()
                .enable_resources_subscribe()
                .enable_resources_list_changed()
                .enable_prompts()
                .enable_prompts_list_changed()
                .enable_tasks()
                .build(),
        )
        .with_protocol_version(ProtocolVersion::V_2026_07_28)
        .with_server_info(Implementation::new(
            "molesignal-inbound-mcp",
            env!("CARGO_PKG_VERSION"),
        ))
        .with_instructions(
            "Manage the authenticated MoleSignal organization. Use tool_search to discover the full catalog, call_read_tool for reads, and call_managed_tool with an idempotency_key for changes. API credentials are never returned over MCP.".to_string(),
        )
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools::protocol_tool(name)
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        tools::list(self, request, &context).await
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        tools::call(self, request, &context).await
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        resources::list(self, request, &context).await
    }

    async fn list_resource_templates(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        resources::list_templates(self, request, &context).await
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        resources::read(self, request, &context).await
    }

    async fn list_prompts(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        prompts::list(self, request, &context).await
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        prompts::get(self, request, &context).await
    }

    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        let mut accepted = requested.supported_by(&self.get_info().capabilities);
        if let Some(uris) = accepted.resource_subscriptions.as_mut() {
            uris.retain(|uri| resources::is_subscribable(uri));
            if uris.is_empty() {
                accepted.resource_subscriptions = None;
            }
        }
        Some(accepted)
    }

    async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
        let data = request_data(self, context.request_context())?;
        for uri in context
            .accepted()
            .resource_subscriptions
            .clone()
            .unwrap_or_default()
        {
            resources::authorize_subscription(self, &data.iam, &uri).await?;
        }
        let mut events = self.runtime.subscribe(&data.iam.org_id);
        let mut resource_refresh = tokio::time::interval(std::time::Duration::from_secs(30));
        resource_refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        resource_refresh.tick().await;
        loop {
            tokio::select! {
                () = context.cancelled() => return Ok(()),
                _ = resource_refresh.tick() => {
                    if context.accepted().tools_list_changed == Some(true) {
                        context.sink().notify_tool_list_changed().await.map_err(subscription_error)?;
                    }
                    if context.accepted().prompts_list_changed == Some(true) {
                        context.sink().notify_prompt_list_changed().await.map_err(subscription_error)?;
                    }
                    if context.accepted().resources_list_changed == Some(true) {
                        context.sink().notify_resource_list_changed().await.map_err(subscription_error)?;
                    }
                    for uri in context.accepted().resource_subscriptions.clone().unwrap_or_default() {
                        context.sink().notify_resource_updated(uri).await.map_err(subscription_error)?;
                    }
                }
                event = events.recv() => match event {
                    Ok(CatalogEvent::Tools) if context.accepted().tools_list_changed == Some(true) => {
                        context.sink().notify_tool_list_changed().await.map_err(subscription_error)?;
                    }
                    Ok(CatalogEvent::Prompts) if context.accepted().prompts_list_changed == Some(true) => {
                        context.sink().notify_prompt_list_changed().await.map_err(subscription_error)?;
                    }
                    Ok(CatalogEvent::Resources) if context.accepted().resources_list_changed == Some(true) => {
                        context.sink().notify_resource_list_changed().await.map_err(subscription_error)?;
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if context.accepted().tools_list_changed == Some(true) {
                            context.sink().notify_tool_list_changed().await.map_err(subscription_error)?;
                        }
                        if context.accepted().prompts_list_changed == Some(true) {
                            context.sink().notify_prompt_list_changed().await.map_err(subscription_error)?;
                        }
                        if context.accepted().resources_list_changed == Some(true) {
                            context.sink().notify_resource_list_changed().await.map_err(subscription_error)?;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
                }
            }
        }
    }

    #[allow(deprecated)]
    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let data = request_data(self, &context)?;
        if !resources::is_subscribable(&request.uri) {
            return Err(ErrorData::resource_not_found(
                format!("resource `{}` is not subscribable", request.uri),
                None,
            ));
        }
        resources::authorize_subscription(self, &data.iam, &request.uri).await?;
        let cancellation = CancellationToken::new();
        if let Some(previous) = self
            .legacy_resource_subscriptions
            .lock()
            .await
            .insert(request.uri.clone(), cancellation.clone())
        {
            previous.cancel();
        }
        let peer = context.peer;
        let uri = request.uri;
        let mut events = self.runtime.subscribe(&data.iam.org_id);
        crate::shared::trace_context::spawn_with_current_trace_context(async move {
            let mut refresh = tokio::time::interval(std::time::Duration::from_secs(30));
            refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            refresh.tick().await;
            loop {
                let notify = tokio::select! {
                    () = cancellation.cancelled() => break,
                    _ = refresh.tick() => true,
                    event = events.recv() => match event {
                        Ok(CatalogEvent::Resources)
                        | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => true,
                        Ok(_) => false,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    },
                };
                if notify
                    && peer
                        .notify_resource_updated(ResourceUpdatedNotificationParam::new(uri.clone()))
                        .await
                        .is_err()
                {
                    break;
                }
            }
        });
        Ok(())
    }

    #[allow(deprecated)]
    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let _ = request_data(self, &context)?;
        if let Some(cancellation) = self
            .legacy_resource_subscriptions
            .lock()
            .await
            .remove(request.uri.as_str())
        {
            cancellation.cancel();
        }
        Ok(())
    }

    async fn get_task(
        &self,
        request: GetTaskParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetTaskResult, ErrorData> {
        tasks::get(self, request, &context).await
    }

    async fn update_task(
        &self,
        request: UpdateTaskParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        tasks::update(self, request, &context).await
    }

    async fn cancel_task(
        &self,
        request: CancelTaskParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        tasks::cancel(self, request, &context).await
    }
}

fn subscription_error(error: impl std::fmt::Display) -> ErrorData {
    ErrorData::internal_error(format!("notification subscription failed: {error}"), None)
}
