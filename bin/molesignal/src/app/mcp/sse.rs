// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 旧版 MCP 传输（HTTP + SSE，协议版本 2024-11-05）。
//!
//! 先 GET 一条事件流，服务器在 `endpoint` 事件里给出发消息的地址；之后每个请求 POST 到那里，
//! 应答从事件流里回来。发消息的地址必须与事件流同源（协议、主机、端口），连接钉在同一组
//! 校验过的地址上。服务器在流里发来的请求：`ping` 回空结果，其余回"不支持"。每个请求的
//! 等待时间与读到的字节数分别受服务器的超时与应答上限约束。

use std::{
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use eventsource_stream::{Event, Eventsource};
use futures::{Stream, StreamExt};
use reqwest::{Client, header::HeaderMap};
use serde_json::{Value, json};
use tokio::time::{Instant, timeout_at};
use url::Url;

use super::transport::{
    client_for, initialize_params, max_bytes_of, request_headers, rpc_result, status_error, target,
    timeout_of,
};
use crate::{
    agent::tool_control::McpServerRuntime,
    app::egress::TargetPolicy,
    shared::{
        Error, Result,
        http_trace::{self, HttpTarget},
        ids::Id,
    },
};

type Events = Pin<Box<dyn Stream<Item = Result<Event>> + Send>>;

/// 发消息的一端。和事件流分开放：事件流不能在线程间共享，发消息时只借用这一端。
struct Outbox {
    client: Client,
    url: Url,
    headers: HeaderMap,
    max_bytes: usize,
}

pub(super) struct SseSession {
    outbox: Outbox,
    events: Events,
    /// 这个请求开始后从事件流读到的字节数。
    read: Arc<AtomicUsize>,
    timeout: Duration,
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default()
}

async fn next_event(events: &mut Events, deadline: Instant) -> Result<Event> {
    match timeout_at(deadline, events.next()).await {
        Err(_) => Err(Error::unavailable("the MCP Server did not answer in time")),
        Ok(None) => Err(Error::unavailable("the MCP Server closed the event stream")),
        Ok(Some(event)) => event,
    }
}

impl SseSession {
    pub(super) async fn open(
        runtime: &McpServerRuntime,
        policy: &TargetPolicy,
    ) -> Result<(Self, Value)> {
        let (url, host, addresses) = target(&runtime.server, policy).await?;
        let client = client_for(&runtime.server, &host, &addresses, true)?;
        let headers = request_headers(runtime)?;
        let timeout = timeout_of(&runtime.server);
        let max_bytes = max_bytes_of(&runtime.server);
        let deadline = Instant::now() + timeout;

        let request = client
            .get(url.clone())
            .headers(headers.clone())
            .header("accept", "text/event-stream");

        let response = timeout_at(
            deadline,
            http_trace::send(&client, request, HttpTarget::ThirdParty),
        )
        .await
        .map_err(|_| Error::unavailable("the MCP Server did not open its event stream in time"))?
        .map_err(|error| Error::unavailable(format!("MCP request failed: {error}")))?;

        if !response.status().is_success() {
            return Err(status_error(response.status()));
        }

        let streams_events = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"));

        if !streams_events {
            return Err(Error::invalid(
                "the MCP Server did not open an event stream; try the Streamable HTTP transport",
            ));
        }

        let read = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&read);

        let events: Events = Box::pin(
            response
                .bytes_stream()
                .map(move |chunk| {
                    let chunk = chunk.map_err(|error| format!("MCP event stream read: {error}"))?;

                    if counter.fetch_add(chunk.len(), Ordering::Relaxed) + chunk.len() > max_bytes {
                        return Err(format!("MCP response exceeds {max_bytes} bytes"));
                    }

                    Ok(chunk)
                })
                .eventsource()
                .map(|event| event.map_err(|error| Error::unavailable(error.to_string()))),
        );

        let mut session = Self {
            outbox: Outbox {
                client,
                url: url.clone(),
                headers,
                max_bytes,
            },
            events,
            read,
            timeout,
        };

        let endpoint = loop {
            let event = next_event(&mut session.events, deadline).await?;

            if event.event == "endpoint" {
                break event.data;
            }
        };

        let post_url = url
            .join(endpoint.trim())
            .map_err(|_| Error::invalid("the MCP Server sent an invalid message endpoint"))?;

        if !same_origin(&url, &post_url) {
            return Err(Error::forbidden(
                "the MCP Server asked for messages on another host, which is not allowed",
            ));
        }

        session.outbox.url = post_url;

        let initialized = session.request("initialize", initialize_params()).await?;

        session
            .outbox
            .post(
                json!({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}}),
                Instant::now() + session.timeout,
            )
            .await?;

        Ok((session, initialized))
    }

    pub(super) async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.read.store(0, Ordering::Relaxed);

        let deadline = Instant::now() + self.timeout;
        let id = Value::String(Id::new().0);

        self.outbox
            .post(
                json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
                deadline,
            )
            .await?;

        loop {
            let event = next_event(&mut self.events, deadline).await?;

            if event.event != "message" {
                continue;
            }

            let Ok(message) = serde_json::from_str::<Value>(&event.data) else {
                continue;
            };

            match (
                message.get("method").and_then(Value::as_str),
                message.get("id"),
            ) {
                (None, Some(answered)) if *answered == id => return rpc_result(method, message),
                (Some(asked), Some(request_id)) => {
                    let reply = if asked == "ping" {
                        json!({"jsonrpc": "2.0", "id": request_id, "result": {}})
                    } else {
                        json!({
                            "jsonrpc": "2.0",
                            "id": request_id,
                            "error": {"code": -32601, "message": "Method not found"}
                        })
                    };

                    self.outbox.post(reply, deadline).await?;
                }
                _ => {}
            }
        }
    }
}

impl Outbox {
    async fn post(&self, body: Value, deadline: Instant) -> Result<()> {
        let request = self
            .client
            .post(self.url.clone())
            .headers(self.headers.clone())
            .header("content-type", "application/json")
            .json(&body);

        let response = timeout_at(
            deadline,
            http_trace::send(&self.client, request, HttpTarget::ThirdParty),
        )
        .await
        .map_err(|_| Error::unavailable("the MCP Server did not accept the message in time"))?
        .map_err(|error| Error::unavailable(format!("MCP request failed: {error}")))?;

        if !response.status().is_success() {
            return Err(status_error(response.status()));
        }

        // 应答在事件流里；这里的正文（通常是 `Accepted`）读掉即可，但不读超过上限的部分。
        let _ =
            super::transport::bounded_response_bytes(response, self.max_bytes.min(64 * 1024)).await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        response::sse::{Event as SseEvent, Sse},
        routing::{get, post},
    };
    use tokio::sync::{Mutex, mpsc};
    use tokio_stream::wrappers::UnboundedReceiverStream;

    use super::*;
    use crate::{
        agent::{scope::Scope, tool_control::McpServer},
        shared::time::TimestampMicros,
    };

    /// What was posted, with the `x-tenant` header it came with.
    type Posted = Arc<Mutex<Vec<(Value, Option<String>)>>>;

    /// A legacy server: one event stream, and the messages posted to it answered on that stream.
    #[derive(Clone)]
    struct Legacy {
        answers: mpsc::UnboundedSender<String>,
        stream: Arc<Mutex<Option<mpsc::UnboundedReceiver<String>>>>,
        posted: Posted,
    }

    async fn events(
        State(legacy): State<Legacy>,
    ) -> Sse<impl Stream<Item = std::result::Result<SseEvent, Infallible>>> {
        let answers = legacy.stream.lock().await.take().expect("one event stream");

        let endpoint = futures::stream::once(async {
            Ok(SseEvent::default()
                .event("endpoint")
                .data("/messages?session=1"))
        });

        let messages = UnboundedReceiverStream::new(answers)
            .map(|data| Ok(SseEvent::default().event("message").data(data)));

        Sse::new(endpoint.chain(messages))
    }

    async fn message(
        State(legacy): State<Legacy>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> StatusCode {
        let tenant = headers
            .get("x-tenant")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);

        legacy.posted.lock().await.push((body.clone(), tenant));

        if let (Some(id), Some(method)) = (body.get("id"), body["method"].as_str()) {
            let result = match method {
                "initialize" => json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "legacy"}
                }),
                "tools/list" => {
                    // A request of the server's own comes first; the client answers it.
                    let ping = json!({"jsonrpc": "2.0", "id": "server-1", "method": "ping"});
                    let _ = legacy.answers.send(ping.to_string());

                    json!({"tools": [{"name": "search", "inputSchema": {"type": "object"}}]})
                }
                _ => json!({}),
            };

            let answer = json!({"jsonrpc": "2.0", "id": id, "result": result});
            let _ = legacy.answers.send(answer.to_string());
        }

        StatusCode::ACCEPTED
    }

    fn server(endpoint: String) -> McpServer {
        McpServer {
            id: Id::from_string("server-1"),
            org_id: Id::from_string("org-1"),
            scope: Scope::Org,
            handle: "legacy".into(),
            name: "Legacy".into(),
            transport: "sse".into(),
            endpoint_url: Some(endpoint),
            command_template: None,
            auth_type: "none".into(),
            auth_header: None,
            credential_last4: None,
            credential_set: false,
            member_credential: None,
            headers: Vec::new(),
            env: Vec::new(),
            private_only: false,
            allowed_domains: Vec::new(),
            allowed_cidrs: Vec::new(),
            follow_redirects: false,
            tls_verify: true,
            timeout_ms: 5_000,
            max_response_bytes: 1_048_576,
            enabled: true,
            status: "healthy".into(),
            last_error: None,
            last_tested_at: None,
            last_synced_at: None,
            created_by: Id::from_string("user-1"),
            created_at: TimestampMicros(1),
            updated_at: TimestampMicros(1),
        }
    }

    #[tokio::test]
    async fn a_legacy_server_answers_on_its_event_stream() {
        let (answers, stream) = mpsc::unbounded_channel();

        let legacy = Legacy {
            answers,
            stream: Arc::new(Mutex::new(Some(stream))),
            posted: Arc::new(Mutex::new(Vec::new())),
        };

        let app = Router::new()
            .route("/sse", get(events))
            .route("/messages", post(message))
            .with_state(legacy.clone());

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();

        tokio::spawn(async move { axum::serve(listener, app).await });

        let runtime = McpServerRuntime {
            server: server(format!("http://{address}/sse")),
            credential: None,
            env: Vec::new(),
            headers: vec![("x-tenant".into(), "acme".into())],
            member_credential: None,
        };

        let (mut session, initialized) = SseSession::open(&runtime, &TargetPolicy::admin_any())
            .await
            .unwrap();

        assert_eq!(initialized["serverInfo"]["name"], "legacy");

        let tools = session.request("tools/list", json!({})).await.unwrap();
        assert_eq!(tools["tools"][0]["name"], "search");

        let posted = legacy.posted.lock().await;

        // The ping of the server was answered, and every message carried the configured header.
        assert!(
            posted
                .iter()
                .any(|(body, _)| body["id"] == "server-1" && body.get("result").is_some())
        );
        assert!(
            posted
                .iter()
                .all(|(_, tenant)| tenant.as_deref() == Some("acme"))
        );
        assert!(
            posted
                .iter()
                .any(|(body, _)| body["method"] == "notifications/initialized")
        );
    }

    #[test]
    fn messages_must_go_back_to_the_same_origin() {
        let stream = Url::parse("https://mcp.example/sse").unwrap();

        assert!(same_origin(
            &stream,
            &stream.join("/messages?sessionId=1").unwrap()
        ));
        assert!(same_origin(
            &stream,
            &Url::parse("https://mcp.example:443/m").unwrap()
        ));
        assert!(!same_origin(
            &stream,
            &stream.join("https://evil.example/messages").unwrap()
        ));
        assert!(!same_origin(
            &stream,
            &Url::parse("http://mcp.example/m").unwrap()
        ));
    }
}
