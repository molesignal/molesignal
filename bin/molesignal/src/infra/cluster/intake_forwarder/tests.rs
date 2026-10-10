// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::json;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{Request, Response, transport::Server};

use super::*;
use crate::{
    domain::intake::RawEvent,
    protocol::intake::v1::{
        IntakeError as ProtoIntakeError,
        intake_service_server::{IntakeService, IntakeServiceServer},
    },
    shared::time::TimestampMicros,
};

/// 记录收到的请求，并按配置应答的 intake 节点。
struct FakeIntake {
    received: Arc<Mutex<Vec<PushRequest>>>,
    reply: std::result::Result<PushResponse, Status>,
}

#[tonic::async_trait]
impl IntakeService for FakeIntake {
    async fn push(
        &self,
        request: Request<PushRequest>,
    ) -> std::result::Result<Response<PushResponse>, Status> {
        self.received.lock().push(request.into_inner());
        self.reply.clone().map(Response::new)
    }
}

async fn serve(
    reply: std::result::Result<PushResponse, Status>,
) -> (String, Arc<Mutex<Vec<PushRequest>>>) {
    let received = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();

    let service = FakeIntake {
        received: received.clone(),
        reply,
    };

    tokio::spawn(
        Server::builder()
            .add_service(IntakeServiceServer::new(service))
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );

    (addr, received)
}

struct Peers(Vec<PeerInfo>);

#[async_trait]
impl ClusterRegistry for Peers {
    async fn list_role(&self, role: PeerRole) -> Vec<PeerInfo> {
        if role == PeerRole::Intake {
            self.0.clone()
        } else {
            Vec::new()
        }
    }
}

fn peer(node_id: &str, addr: &str) -> PeerInfo {
    PeerInfo {
        node_id: node_id.into(),
        advertise_addr: addr.into(),
        roles: vec![PeerRole::Intake],
    }
}

fn batch(stream_type: StreamType) -> IntakeBatch {
    IntakeBatch {
        batch_id: Id::from_string("batch-1"),
        org_id: Id::from_string("org-1"),
        stream: "app".into(),
        stream_type,
        events: vec![RawEvent {
            timestamp: TimestampMicros(1_700_000_000_000_000),
            fields: match json!({"level": "error"}) {
                serde_json::Value::Object(map) => map,
                _ => unreachable!(),
            },
        }],
        received_at: TimestampMicros(1_700_000_000_500_000),
    }
}

fn accepted(count: u32) -> PushResponse {
    PushResponse {
        accepted: count,
        rejected: 0,
        errors: Vec::new(),
    }
}

#[tokio::test]
async fn forwards_the_batch_unchanged_and_returns_the_intake_result() {
    let reply = PushResponse {
        accepted: 1,
        rejected: 1,
        errors: vec![ProtoIntakeError {
            index: 3,
            reason: "bad field".into(),
        }],
    };
    let (addr, received) = serve(Ok(reply)).await;
    let forwarder = GrpcIntakeForwarder::new(Arc::new(Peers(vec![peer("n1", &addr)])));

    let result = forwarder.forward(batch(StreamType::LOGS)).await.unwrap();

    assert_eq!((result.accepted, result.rejected), (1, 1));
    assert_eq!(result.errors[0].index, 3);
    assert_eq!(result.errors[0].reason, "bad field");
    let received = received.lock();
    let request = &received[0];
    assert_eq!(request.batch_id, "batch-1");
    assert_eq!(request.org_id, "org-1");
    assert_eq!(request.stream, "app");
    assert_eq!(request.stream_type, ProtoStreamType::Logs as i32);
    assert_eq!(request.stream_type_name, "logs");
    assert_eq!(request.received_at_micros, 1_700_000_000_500_000);
    let events: Vec<RawEvent> = serde_json::from_slice(&request.payload).unwrap();
    assert_eq!(events[0].fields["level"], "error");
}

#[tokio::test]
async fn stream_types_outside_the_proto_enum_travel_by_name() {
    let (addr, received) = serve(Ok(accepted(1))).await;
    let forwarder = GrpcIntakeForwarder::new(Arc::new(Peers(vec![peer("n1", &addr)])));

    forwarder.forward(batch(StreamType::EXTEND)).await.unwrap();

    let received = received.lock();
    assert_eq!(received[0].stream_type, ProtoStreamType::Unspecified as i32);
    assert_eq!(received[0].stream_type_name, "extend");
}

#[tokio::test]
async fn an_unreachable_node_is_skipped() {
    let (live, received) = serve(Ok(accepted(1))).await;
    let registry = Peers(vec![peer("down", "127.0.0.1:1"), peer("up", &live)]);
    let forwarder = GrpcIntakeForwarder::new(Arc::new(registry));

    let result = forwarder.forward(batch(StreamType::LOGS)).await.unwrap();

    assert_eq!(result.accepted, 1);
    assert_eq!(received.lock().len(), 1);
}

#[tokio::test]
async fn nodes_that_answer_with_an_error_are_not_retried_elsewhere() {
    let (first, first_received) = serve(Err(Status::invalid_argument("bad payload"))).await;
    let (second, second_received) = serve(Ok(accepted(1))).await;
    let registry = Peers(vec![peer("a", &first), peer("b", &second)]);
    let forwarder = GrpcIntakeForwarder::new(Arc::new(registry));

    let error = forwarder
        .forward(batch(StreamType::LOGS))
        .await
        .unwrap_err();

    assert_eq!(error.http_status_code(), 400);
    assert_eq!(first_received.lock().len(), 1);
    assert!(second_received.lock().is_empty());
}

#[tokio::test]
async fn peer_statuses_map_to_the_matching_error_kinds() {
    for (status, expected) in [
        (Status::resource_exhausted("slow down"), 429),
        (Status::unavailable("draining"), 503),
        (Status::permission_denied("no"), 403),
        (Status::unauthenticated("no"), 401),
        (Status::out_of_range("too big"), 413),
        (Status::aborted("conflict"), 409),
        (Status::internal("boom"), 500),
    ] {
        let (addr, _) = serve(Err(status)).await;
        let forwarder = GrpcIntakeForwarder::new(Arc::new(Peers(vec![peer("n1", &addr)])));
        let error = forwarder
            .forward(batch(StreamType::LOGS))
            .await
            .unwrap_err();
        assert_eq!(error.http_status_code(), expected);
    }
}

#[tokio::test]
async fn no_registered_intake_node_is_an_unavailable_error() {
    let forwarder = GrpcIntakeForwarder::new(Arc::new(Peers(Vec::new())));
    let error = forwarder
        .forward(batch(StreamType::LOGS))
        .await
        .unwrap_err();
    assert_eq!(error.http_status_code(), 503);
}
