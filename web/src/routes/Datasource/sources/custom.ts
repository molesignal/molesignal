/** Intake by protocol, for whatever no dedicated guide covers: OpenTelemetry, curl, bulk NDJSON. */

import { ENDPOINT, ENDPOINT_HOST, TOKEN, type Source } from './types';

export const CUSTOM_SOURCES: Source[] = [
  {
    id: 'curl',
    category: 'custom',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `curl -X POST ${ENDPOINT}/api/v1/intake/logs/default \\
  -H "Authorization: Bearer ${TOKEN}" \\
  -H "Content-Type: application/json" \\
  -d '[{"level":"info","service":"my-app","message":"hello MoleSignal"}]'`,
        },
      },
      { code: { lang: 'json', content: '{"accepted":1,"rejected":0}' } },
    ],
  },
  {
    id: 'bulk-ndjson',
    category: 'custom',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `curl -X POST ${ENDPOINT}/api/v1/_bulk \\
  -H "Authorization: Bearer ${TOKEN}" \\
  -H "Content-Type: application/x-ndjson" \\
  --data-binary @events.ndjson

# events.ndjson looks like:
# {"index":{"_index":"app-logs"}}
# {"level":"info","message":"a"}`,
        },
      },
      {
        code: {
          lang: 'bash',
          content: `curl -X POST ${ENDPOINT}/api/v1/intake/logs/default \\
  -H "Authorization: Bearer ${TOKEN}" \\
  -H "Content-Type: application/json" \\
  -d '[{"level":"info","message":"a"},{"level":"warn","message":"b"}]'`,
        },
      },
    ],
  },
  {
    id: 'opentelemetry',
    category: 'otel',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      {
        code: {
          lang: 'text',
          content: `HTTP Endpoint: ${ENDPOINT}/api
Authorization: Bearer ${TOKEN}
stream-name:   default`,
        },
      },
      {
        code: {
          lang: 'bash',
          content: `export OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf
export OTEL_EXPORTER_OTLP_ENDPOINT=${ENDPOINT}/api
export OTEL_EXPORTER_OTLP_HEADERS="authorization=Bearer ${TOKEN}"
export OTEL_RESOURCE_ATTRIBUTES="service.name=my-svc,deployment.environment=production"`,
        },
      },
      {
        code: {
          lang: 'bash',
          content: `export OTEL_EXPORTER_OTLP_PROTOCOL=grpc
export OTEL_EXPORTER_OTLP_ENDPOINT=http://${ENDPOINT_HOST}:4317
export OTEL_EXPORTER_OTLP_HEADERS="authorization=Bearer ${TOKEN}"`,
        },
      },
    ],
  },
  {
    id: 'otel-collector',
    category: 'otel-collector',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      {
        code: {
          lang: 'yaml',
          content: `exporters:
  otlphttp/molesignal:
    endpoint: ${ENDPOINT}/api
    headers:
      authorization: "Bearer ${TOKEN}"

service:
  pipelines:
    traces:
      exporters: [otlphttp/molesignal]
    metrics:
      exporters: [otlphttp/molesignal]
    logs:
      exporters: [otlphttp/molesignal]`,
        },
      },
      {
        code: {
          lang: 'yaml',
          content: `exporters:
  otlp/molesignal:
    endpoint: ${ENDPOINT_HOST}:4317
    headers:
      authorization: "Bearer ${TOKEN}"
    tls:
      insecure: true

service:
  pipelines:
    traces:
      exporters: [otlp/molesignal]
    metrics:
      exporters: [otlp/molesignal]
    logs:
      exporters: [otlp/molesignal]`,
        },
      },
    ],
  },
];
