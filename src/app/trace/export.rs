// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! CanonicalSpan → OTLP gRPC / HTTP-protobuf 的显式外部 sink。

use std::{collections::BTreeMap, io::Write, sync::Arc, time::Duration};

use async_trait::async_trait;
use http::{HeaderMap, HeaderName, HeaderValue};
use opentelemetry_proto::tonic::{
    collector::trace::v1::{ExportTraceServiceRequest, trace_service_client::TraceServiceClient},
    common::v1::{AnyValue, ArrayValue, InstrumentationScope, KeyValue, KeyValueList, any_value},
    resource::v1::Resource,
    trace::v1::{ResourceSpans, ScopeSpans, Span, Status, span},
};
use prost::Message;
use reqwest::Client;
use serde_json::{Value, json};
use tonic::{
    codec::CompressionEncoding,
    metadata::{Ascii, MetadataKey, MetadataValue},
    transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity},
};

use crate::{
    app::trace::TraceSink,
    config::ExternalTraceExporterSettings,
    shared::{
        self_telemetry::with_suppression, tail_sampling::DecidedTrace,
        trace_normalization::CanonicalSpan,
    },
};

pub const SELF_EXPORT_MARKER: &str = "x-molesignal-trace-self-export";

enum ExternalOtlpTransport {
    Grpc {
        channel: Channel,
        metadata: Vec<(MetadataKey<Ascii>, MetadataValue<Ascii>)>,
        gzip: bool,
        self_export: bool,
    },
    Http {
        client: Client,
        endpoint: String,
        headers: HeaderMap,
        gzip: bool,
        self_export: bool,
    },
}

pub struct ExternalOtlpTraceSink {
    transport: ExternalOtlpTransport,
}

impl ExternalOtlpTraceSink {
    pub fn new(
        settings: &ExternalTraceExporterSettings,
        local_endpoints: &[String],
    ) -> Result<Option<Arc<Self>>, String> {
        if settings.endpoint.trim().is_empty() {
            return Ok(None);
        }
        let endpoint = url::Url::parse(&settings.endpoint)
            .map_err(|error| format!("invalid Trace OTLP endpoint: {error}"))?;
        let self_export = endpoint_matches_local(&endpoint, local_endpoints);
        if self_export && !settings.allow_self_export {
            return Err(
                "external Trace OTLP endpoint resolves to this MoleSignal cluster; \
                 set allow_self_export=true only for an audited exception"
                    .into(),
            );
        }
        let headers = resolve_headers(&settings.headers)?;
        let timeout = Duration::from_millis(settings.timeout_ms);

        let transport = match settings.protocol.as_str() {
            "grpc" => {
                if endpoint.path() != "/" && !endpoint.path().is_empty() {
                    return Err("gRPC Trace OTLP endpoint must not contain a URL path".into());
                }
                let mut grpc_endpoint = Endpoint::from_shared(settings.endpoint.clone())
                    .map_err(|error| format!("invalid gRPC Trace endpoint: {error}"))?
                    .timeout(timeout)
                    .connect_timeout(timeout);
                if endpoint.scheme() == "https" {
                    let mut tls = ClientTlsConfig::new();
                    if let Some(path) = &settings.custom_ca_file {
                        tls = tls.ca_certificate(Certificate::from_pem(read_secret_file(path)?));
                    }
                    if let (Some(cert), Some(key)) =
                        (&settings.client_certificate_file, &settings.client_key_file)
                    {
                        tls = tls.identity(Identity::from_pem(
                            read_secret_file(cert)?,
                            read_secret_file(key)?,
                        ));
                    }
                    grpc_endpoint = grpc_endpoint
                        .tls_config(tls)
                        .map_err(|error| format!("invalid Trace exporter TLS config: {error}"))?;
                } else if settings.custom_ca_file.is_some()
                    || settings.client_certificate_file.is_some()
                {
                    return Err("custom CA/mTLS requires an https Trace endpoint".into());
                }
                let metadata = headers
                    .iter()
                    .map(|(name, value)| {
                        Ok((
                            MetadataKey::from_bytes(name.as_str().as_bytes())
                                .map_err(|error| format!("invalid gRPC metadata name: {error}"))?,
                            MetadataValue::<Ascii>::try_from(
                                value
                                    .to_str()
                                    .map_err(|_| "non-ASCII Trace exporter header".to_string())?,
                            )
                            .map_err(|error| format!("invalid gRPC metadata value: {error}"))?,
                        ))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                ExternalOtlpTransport::Grpc {
                    channel: grpc_endpoint.connect_lazy(),
                    metadata,
                    gzip: settings.gzip,
                    self_export,
                }
            }
            "http/protobuf" => {
                let mut builder = Client::builder().timeout(timeout);
                if let Some(path) = &settings.custom_ca_file {
                    let certificate = reqwest::Certificate::from_pem(&read_secret_file(path)?)
                        .map_err(|error| format!("invalid Trace exporter CA: {error}"))?;
                    builder = builder.add_root_certificate(certificate);
                }
                if let (Some(cert), Some(key)) =
                    (&settings.client_certificate_file, &settings.client_key_file)
                {
                    let mut pem = read_secret_file(cert)?;
                    pem.extend_from_slice(&read_secret_file(key)?);
                    let identity = reqwest::Identity::from_pem(&pem).map_err(|error| {
                        format!("invalid Trace exporter mTLS identity: {error}")
                    })?;
                    builder = builder.identity(identity);
                }
                let client = builder
                    .build()
                    .map_err(|error| format!("build Trace exporter HTTP client: {error}"))?;
                let mut endpoint = endpoint;
                if endpoint.path().is_empty() || endpoint.path() == "/" {
                    endpoint.set_path("/v1/traces");
                }
                ExternalOtlpTransport::Http {
                    client,
                    endpoint: endpoint.to_string(),
                    headers,
                    gzip: settings.gzip,
                    self_export,
                }
            }
            protocol => {
                return Err(format!(
                    "unsupported Trace OTLP protocol `{protocol}`; expected grpc or http/protobuf"
                ));
            }
        };
        Ok(Some(Arc::new(Self { transport })))
    }
}

#[async_trait]
impl TraceSink for ExternalOtlpTraceSink {
    fn name(&self) -> &'static str {
        "external_otlp"
    }

    async fn export(&self, traces: &[DecidedTrace]) -> Result<(), String> {
        let request = canonical_to_otlp_request(traces);
        match &self.transport {
            ExternalOtlpTransport::Grpc {
                channel,
                metadata,
                gzip,
                self_export,
            } => {
                let mut request = tonic::Request::new(request);
                for (name, value) in metadata {
                    request.metadata_mut().insert(name.clone(), value.clone());
                }
                if *self_export {
                    request.metadata_mut().insert(
                        SELF_EXPORT_MARKER,
                        "1".parse().expect("static metadata is valid"),
                    );
                }
                let mut client = TraceServiceClient::new(channel.clone());
                if *gzip {
                    client = client.send_compressed(CompressionEncoding::Gzip);
                }
                with_suppression(crate::shared::grpc_trace::call(
                    request,
                    "opentelemetry.proto.collector.trace.v1.TraceService",
                    "Export",
                    crate::shared::grpc_trace::GrpcTarget::ThirdParty,
                    |request| client.export(request),
                ))
                .await
                .map_err(|status| format!("OTLP gRPC export failed: {}", status.code()))?;
                Ok(())
            }
            ExternalOtlpTransport::Http {
                client,
                endpoint,
                headers,
                gzip,
                self_export,
            } => {
                let encoded = request.encode_to_vec();
                let (body, content_encoding) = if *gzip {
                    let mut encoder =
                        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                    encoder
                        .write_all(&encoded)
                        .map_err(|error| format!("gzip Trace batch: {error}"))?;
                    (
                        encoder
                            .finish()
                            .map_err(|error| format!("finish Trace gzip batch: {error}"))?,
                        Some("gzip"),
                    )
                } else {
                    (encoded, None)
                };
                let mut request = client
                    .post(endpoint)
                    .headers(headers.clone())
                    .header(http::header::CONTENT_TYPE, "application/x-protobuf")
                    .body(body);
                if let Some(content_encoding) = content_encoding {
                    request = request.header(http::header::CONTENT_ENCODING, content_encoding);
                }
                if *self_export {
                    request = request.header(SELF_EXPORT_MARKER, "1");
                }
                let response = with_suppression(crate::shared::http_trace::send(
                    client,
                    request,
                    crate::shared::http_trace::HttpTarget::ThirdParty,
                ))
                .await
                .map_err(|error| format!("OTLP HTTP export failed: {error}"))?;
                if response.status().is_success() {
                    Ok(())
                } else {
                    Err(format!("OTLP HTTP export returned {}", response.status()))
                }
            }
        }
    }
}

pub fn canonical_to_otlp_request(traces: &[DecidedTrace]) -> ExportTraceServiceRequest {
    ExportTraceServiceRequest {
        resource_spans: traces
            .iter()
            .flat_map(|trace| &trace.spans)
            .map(canonical_to_resource_spans)
            .collect(),
    }
}

fn canonical_to_resource_spans(canonical: &CanonicalSpan) -> ResourceSpans {
    let mut span_attributes = canonical.attributes.clone();
    span_attributes.insert(
        "molesignal.trace.sampling_reason".into(),
        json!(canonical.sampling_reason),
    );
    span_attributes.insert("molesignal.trace.partial".into(), json!(canonical.partial));
    if !canonical.partial_reasons.is_empty() {
        span_attributes.insert(
            "molesignal.trace.partial_reasons".into(),
            json!(canonical.partial_reasons),
        );
    }
    if canonical.late {
        span_attributes.insert("molesignal.trace.late".into(), json!(true));
    }
    if canonical.conflict {
        span_attributes.insert("molesignal.trace.conflict".into(), json!(true));
    }
    let proto_span = Span {
        trace_id: hex::decode(&canonical.trace_id).unwrap_or_default(),
        span_id: hex::decode(&canonical.span_id).unwrap_or_default(),
        trace_state: canonical.trace_state.clone(),
        parent_span_id: canonical
            .parent_span_id
            .as_deref()
            .and_then(|value| hex::decode(value).ok())
            .unwrap_or_default(),
        flags: canonical.trace_flags,
        name: canonical.name.clone(),
        kind: canonical.kind,
        start_time_unix_nano: canonical.start_time_unix_nano,
        end_time_unix_nano: canonical.end_time_unix_nano,
        attributes: btree_to_key_values(&span_attributes),
        dropped_attributes_count: canonical.dropped_attributes_count,
        events: canonical
            .events
            .iter()
            .map(|event| span::Event {
                time_unix_nano: event.time_unix_nano,
                name: event.name.clone(),
                attributes: btree_to_key_values(&event.attributes),
                dropped_attributes_count: event.dropped_attributes_count,
            })
            .collect(),
        dropped_events_count: canonical.dropped_events_count,
        links: canonical
            .links
            .iter()
            .map(|link| span::Link {
                trace_id: hex::decode(&link.trace_id).unwrap_or_default(),
                span_id: hex::decode(&link.span_id).unwrap_or_default(),
                trace_state: link.trace_state.clone(),
                attributes: btree_to_key_values(&link.attributes),
                dropped_attributes_count: link.dropped_attributes_count,
                flags: link.flags,
            })
            .collect(),
        dropped_links_count: canonical.dropped_links_count,
        status: Some(Status {
            message: canonical.status_message.clone().unwrap_or_default(),
            code: if canonical.status_code.eq_ignore_ascii_case("error") {
                2
            } else if canonical.status_code.eq_ignore_ascii_case("ok") {
                1
            } else {
                0
            },
        }),
    };
    ResourceSpans {
        resource: Some(Resource {
            attributes: btree_to_key_values(&canonical.resource.attributes),
            dropped_attributes_count: canonical.resource.dropped_attributes_count,
            entity_refs: Vec::new(),
        }),
        scope_spans: vec![ScopeSpans {
            scope: Some(InstrumentationScope {
                name: canonical.scope.name.clone(),
                version: canonical.scope.version.clone(),
                attributes: btree_to_key_values(&canonical.scope.attributes),
                dropped_attributes_count: canonical.scope.dropped_attributes_count,
            }),
            spans: vec![proto_span],
            schema_url: canonical.scope.schema_url.clone().unwrap_or_default(),
        }],
        schema_url: canonical.resource.schema_url.clone().unwrap_or_default(),
    }
}

fn btree_to_key_values(values: &BTreeMap<String, Value>) -> Vec<KeyValue> {
    values
        .iter()
        .map(|(key, value)| KeyValue {
            key: key.clone(),
            value: Some(json_to_any_value(value)),
            key_strindex: 0,
        })
        .collect()
}

fn json_to_any_value(value: &Value) -> AnyValue {
    let value = match value {
        Value::Null => any_value::Value::StringValue(String::new()),
        Value::Bool(value) => any_value::Value::BoolValue(*value),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                any_value::Value::IntValue(value)
            } else if let Some(value) = value.as_u64() {
                any_value::Value::IntValue(i64::try_from(value).unwrap_or(i64::MAX))
            } else {
                any_value::Value::DoubleValue(value.as_f64().unwrap_or_default())
            }
        }
        Value::String(value) => any_value::Value::StringValue(value.clone()),
        Value::Array(values) => any_value::Value::ArrayValue(ArrayValue {
            values: values.iter().map(json_to_any_value).collect(),
        }),
        Value::Object(values) => any_value::Value::KvlistValue(KeyValueList {
            values: values
                .iter()
                .map(|(key, value)| KeyValue {
                    key: key.clone(),
                    value: Some(json_to_any_value(value)),
                    key_strindex: 0,
                })
                .collect(),
        }),
    };
    AnyValue { value: Some(value) }
}

fn resolve_headers(config: &BTreeMap<String, String>) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();
    for (name, configured) in config {
        let name = HeaderName::try_from(name)
            .map_err(|error| format!("invalid Trace exporter header name: {error}"))?;
        let resolved = if let Some(variable) = configured.strip_prefix("env:") {
            std::env::var(variable).map_err(|_| {
                format!("Trace exporter environment reference `{variable}` is unset")
            })?
        } else if let Some(reference) = configured.strip_prefix("secret:") {
            let variable = format!(
                "MS_SECRET_{}",
                reference
                    .chars()
                    .map(|character| {
                        if character.is_ascii_alphanumeric() {
                            character.to_ascii_uppercase()
                        } else {
                            '_'
                        }
                    })
                    .collect::<String>()
            );
            std::env::var(&variable).map_err(|_| {
                format!("Trace exporter secret reference `{reference}` could not be resolved")
            })?
        } else {
            configured.clone()
        };
        let value = HeaderValue::from_str(&resolved)
            .map_err(|error| format!("invalid Trace exporter header value: {error}"))?;
        headers.insert(name, value);
    }
    Ok(headers)
}

fn read_secret_file(path: &str) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("read Trace exporter TLS file `{path}`: {error}"))
}

fn endpoint_matches_local(endpoint: &url::Url, local_endpoints: &[String]) -> bool {
    local_endpoints.iter().any(|candidate| {
        url::Url::parse(candidate).ok().is_some_and(|local| {
            endpoint.host_str() == local.host_str()
                && endpoint.port_or_known_default() == local.port_or_known_default()
        })
    })
}

#[cfg(test)]
mod tests {
    use std::{
        io::Read as _,
        sync::{Arc, Mutex},
    };

    use opentelemetry_proto::tonic::collector::trace::v1::{
        ExportTraceServiceResponse,
        trace_service_server::{TraceService, TraceServiceServer},
    };
    use rcgen::{
        BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
        KeyUsagePurpose,
    };
    use tokio_stream::wrappers::TcpListenerStream;
    use tonic::{Request, Response, Status, transport::Server};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path},
    };

    use super::*;
    use crate::{
        app::trace::{MemoryTraceSink, TracePipeline, TracePipelineConfig, TraceSinkWorkerConfig},
        shared::{
            tail_sampling::{
                DecidedTrace, ForceKeep, TailSampler, TraceCandidate, TraceRuntimePolicy,
            },
            trace_fixtures,
            trace_normalization::{SamplingReason, TraceLimits},
        },
    };

    fn retained_trace() -> DecidedTrace {
        let spans = trace_fixtures::canonical_async_link_trace();
        DecidedTrace {
            org_id: "org".into(),
            stream: None,
            trace_id: spans[1].trace_id.clone(),
            policy_version: TraceRuntimePolicy::default().version,
            kept: true,
            reason: SamplingReason::Ratio,
            spans,
        }
    }

    #[test]
    fn canonical_otlp_round_trip_preserves_nested_fields() {
        let trace = retained_trace();
        let request = canonical_to_otlp_request(&[trace]);
        assert_eq!(request.resource_spans.len(), 2);
        assert_eq!(
            request.resource_spans[1].scope_spans[0].spans[0]
                .links
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn http_protobuf_collector_receives_auth_gzip_and_complete_batch() {
        let collector = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/traces"))
            .and(header("x-collector-token", "fixture-token"))
            .and(header("content-type", "application/x-protobuf"))
            .and(header("content-encoding", "gzip"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&collector)
            .await;
        let mut settings = ExternalTraceExporterSettings {
            endpoint: collector.uri(),
            protocol: "http/protobuf".into(),
            gzip: true,
            ..ExternalTraceExporterSettings::default()
        };
        settings
            .headers
            .insert("x-collector-token".into(), "fixture-token".into());
        let sink = ExternalOtlpTraceSink::new(&settings, &[])
            .expect("build HTTP exporter")
            .expect("enabled HTTP exporter");

        sink.export(&[retained_trace()])
            .await
            .expect("HTTP/protobuf export");

        let requests = collector
            .received_requests()
            .await
            .expect("request recording enabled");
        assert_eq!(requests.len(), 1);
        let mut body = Vec::new();
        flate2::read::GzDecoder::new(requests[0].body.as_slice())
            .read_to_end(&mut body)
            .expect("decode gzip Trace body");
        let request =
            ExportTraceServiceRequest::decode(body.as_slice()).expect("decode OTLP protobuf");
        assert_eq!(request.resource_spans.len(), 2);
    }

    #[tokio::test]
    async fn pipeline_prevents_forbidden_content_from_reaching_self_or_external_sink() {
        let collector = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/traces"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&collector)
            .await;
        let settings = ExternalTraceExporterSettings {
            endpoint: collector.uri(),
            protocol: "http/protobuf".into(),
            ..ExternalTraceExporterSettings::default()
        };
        let external = ExternalOtlpTraceSink::new(&settings, &[])
            .expect("build HTTP exporter")
            .expect("enabled HTTP exporter");
        let self_ingest = Arc::new(MemoryTraceSink::default());
        let sampler = Arc::new(
            TailSampler::new(
                TraceRuntimePolicy {
                    normal_sample_ratio: 1.0,
                    root_grace_ms: 1,
                    ..TraceRuntimePolicy::default()
                },
                false,
                TraceLimits::default(),
            )
            .unwrap(),
        );
        let sink = TraceSinkWorkerConfig {
            queue_capacity: 8,
            batch_size: 8,
            batch_delay: Duration::from_millis(1),
            export_timeout: Duration::from_secs(1),
            max_attempts: 1,
            initial_backoff: Duration::from_millis(1),
        };
        let pipeline = TracePipeline::start(
            sampler,
            Some(self_ingest.clone()),
            Some(external),
            TracePipelineConfig {
                candidate_capacity: 8,
                decision_tick: Duration::from_millis(1),
                shutdown_timeout: Duration::from_secs(2),
                self_ingest: sink,
                external: sink,
            },
            TraceLimits::default(),
        )
        .unwrap();
        let mut span = trace_fixtures::canonical_http_trace().remove(0);
        span.attributes.insert(
            "authorization".into(),
            serde_json::json!("Bearer trace-pipeline-secret"),
        );
        span.attributes.insert(
            "nested".into(),
            serde_json::json!({
                "password": "trace-pipeline-password",
                "message": "trace-private@example.invalid"
            }),
        );
        pipeline
            .try_submit(TraceCandidate {
                org_id: "org".into(),
                stream: None,
                span,
                force_keep: ForceKeep::TrustedInternal,
            })
            .unwrap();
        pipeline.shutdown().await;

        let stored_traces = self_ingest.traces("org").await;
        let stored_spans = stored_traces
            .iter()
            .flat_map(|trace| trace.spans.iter())
            .collect::<Vec<_>>();
        let stored = serde_json::to_vec(&stored_spans).unwrap();
        let requests = collector
            .received_requests()
            .await
            .expect("request recording enabled");
        assert_eq!(requests.len(), 1);
        for forbidden in [
            b"trace-pipeline-secret".as_slice(),
            b"trace-pipeline-password".as_slice(),
            b"trace-private@example.invalid".as_slice(),
        ] {
            assert!(
                !stored
                    .windows(forbidden.len())
                    .any(|window| window == forbidden)
            );
            assert!(
                !requests[0]
                    .body
                    .windows(forbidden.len())
                    .any(|window| window == forbidden)
            );
        }
    }

    type RecordedGrpcRequest = (Option<String>, ExportTraceServiceRequest);

    #[derive(Clone, Default)]
    struct GrpcCollector {
        requests: Arc<Mutex<Vec<RecordedGrpcRequest>>>,
    }

    #[tonic::async_trait]
    impl TraceService for GrpcCollector {
        async fn export(
            &self,
            request: Request<ExportTraceServiceRequest>,
        ) -> Result<Response<ExportTraceServiceResponse>, Status> {
            let token = request
                .metadata()
                .get("x-collector-token")
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned);
            self.requests
                .lock()
                .expect("collector request lock")
                .push((token, request.into_inner()));
            Ok(Response::new(ExportTraceServiceResponse::default()))
        }
    }

    #[tokio::test]
    async fn grpc_collector_receives_metadata_gzip_and_complete_batch() {
        let collector = GrpcCollector::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind mock OTLP collector");
        let address = listener.local_addr().expect("mock collector address");
        let server_collector = collector.clone();
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(
                    TraceServiceServer::new(server_collector)
                        .accept_compressed(CompressionEncoding::Gzip),
                )
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
        });
        let mut settings = ExternalTraceExporterSettings {
            endpoint: format!("http://{address}"),
            protocol: "grpc".into(),
            gzip: true,
            ..ExternalTraceExporterSettings::default()
        };
        settings
            .headers
            .insert("x-collector-token".into(), "fixture-token".into());
        let sink = ExternalOtlpTraceSink::new(&settings, &[])
            .expect("build gRPC exporter")
            .expect("enabled gRPC exporter");

        sink.export(&[retained_trace()])
            .await
            .expect("gRPC OTLP export");

        let requests = collector.requests.lock().expect("collector request lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].0.as_deref(), Some("fixture-token"));
        assert_eq!(requests[0].1.resource_spans.len(), 2);
        drop(requests);
        server.abort();
    }

    #[tokio::test]
    async fn grpc_collector_requires_and_accepts_custom_ca_mtls_identity() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let mut ca_params =
            CertificateParams::new(Vec::<String>::new()).expect("empty CA subject names");
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        let ca_key = KeyPair::generate().expect("generate CA key");
        let ca_certificate = ca_params.self_signed(&ca_key).expect("self-sign CA");
        let ca_pem = ca_certificate.pem();
        let ca_issuer = Issuer::new(ca_params, ca_key);

        let mut server_params =
            CertificateParams::new(vec!["127.0.0.1".into()]).expect("server certificate params");
        server_params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ServerAuth);
        let server_key = KeyPair::generate().expect("generate server key");
        let server_certificate = server_params
            .signed_by(&server_key, &ca_issuer)
            .expect("sign server certificate");

        let mut client_params =
            CertificateParams::new(Vec::<String>::new()).expect("client certificate params");
        client_params
            .extended_key_usages
            .push(ExtendedKeyUsagePurpose::ClientAuth);
        let client_key = KeyPair::generate().expect("generate client key");
        let client_certificate = client_params
            .signed_by(&client_key, &ca_issuer)
            .expect("sign client certificate");

        let collector = GrpcCollector::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind TLS mock OTLP collector");
        let address = listener.local_addr().expect("TLS mock collector address");
        let server_collector = collector.clone();
        let server_tls = tonic::transport::ServerTlsConfig::new()
            .identity(Identity::from_pem(
                server_certificate.pem(),
                server_key.serialize_pem(),
            ))
            .client_ca_root(Certificate::from_pem(ca_pem.clone()));
        let server = tokio::spawn(async move {
            Server::builder()
                .tls_config(server_tls)
                .expect("valid mock collector TLS")
                .add_service(TraceServiceServer::new(server_collector))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
        });

        let fixture_dir = tempfile::tempdir().expect("mTLS fixture directory");
        let ca_path = fixture_dir.path().join("ca.pem");
        let certificate_path = fixture_dir.path().join("client.pem");
        let key_path = fixture_dir.path().join("client-key.pem");
        std::fs::write(&ca_path, ca_pem).expect("write CA fixture");
        std::fs::write(&certificate_path, client_certificate.pem())
            .expect("write client certificate fixture");
        std::fs::write(&key_path, client_key.serialize_pem()).expect("write client key fixture");
        let settings = ExternalTraceExporterSettings {
            endpoint: format!("https://{address}"),
            protocol: "grpc".into(),
            custom_ca_file: Some(ca_path.to_string_lossy().into_owned()),
            client_certificate_file: Some(certificate_path.to_string_lossy().into_owned()),
            client_key_file: Some(key_path.to_string_lossy().into_owned()),
            ..ExternalTraceExporterSettings::default()
        };
        let sink = ExternalOtlpTraceSink::new(&settings, &[])
            .expect("build mTLS gRPC exporter")
            .expect("enabled mTLS gRPC exporter");

        sink.export(&[retained_trace()])
            .await
            .expect("mTLS gRPC OTLP export");

        assert_eq!(
            collector
                .requests
                .lock()
                .expect("collector request lock")
                .len(),
            1
        );
        server.abort();
    }

    #[test]
    fn self_export_loop_is_rejected_by_default() {
        let settings = ExternalTraceExporterSettings {
            endpoint: "http://127.0.0.1:4317".into(),
            ..ExternalTraceExporterSettings::default()
        };
        let result = ExternalOtlpTraceSink::new(&settings, &["http://127.0.0.1:4317".into()]);
        assert!(result.is_err());
    }

    #[test]
    fn protocol_is_never_guessed_from_url() {
        let settings = ExternalTraceExporterSettings {
            endpoint: "http://collector.example/v1/traces".into(),
            protocol: "grpc".into(),
            ..ExternalTraceExporterSettings::default()
        };
        let result = ExternalOtlpTraceSink::new(&settings, &[]);
        assert!(
            result.is_err(),
            "gRPC rejects path instead of guessing HTTP"
        );
    }
}
