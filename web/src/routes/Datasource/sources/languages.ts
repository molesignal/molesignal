/** Application SDKs, one per language. */

import { ENDPOINT, TOKEN, type Source } from './types';

export const LANGUAGE_SOURCES: Source[] = [
  {
    id: 'python',
    category: 'languages',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      { code: { lang: 'bash', content: 'pip install opentelemetry-distro opentelemetry-exporter-otlp\nopentelemetry-bootstrap -a install' } },
      {
        code: {
          lang: 'bash',
          content: `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf \\
OTEL_EXPORTER_OTLP_ENDPOINT=${ENDPOINT}/api \\
OTEL_EXPORTER_OTLP_HEADERS="authorization=Bearer ${TOKEN}" \\
OTEL_SERVICE_NAME=my-svc \\
opentelemetry-instrument python app.py`,
        },
      },
    ],
  },
  {
    id: 'go',
    category: 'languages',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      {
        code: {
          lang: 'go',
          content: `import (
  "go.opentelemetry.io/otel"
  "go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp"
  "go.opentelemetry.io/otel/sdk/trace"
)

exp, _ := otlptracehttp.New(ctx,
  otlptracehttp.WithEndpointURL("${ENDPOINT}/api/v1/traces"),
  otlptracehttp.WithHeaders(map[string]string{"authorization": "Bearer ${TOKEN}"}),
)
tp := trace.NewTracerProvider(trace.WithBatcher(exp))
otel.SetTracerProvider(tp)`,
        },
      },
    ],
  },
  {
    id: 'java',
    category: 'languages',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `java -javaagent:opentelemetry-javaagent.jar \\
  -Dotel.service.name=my-svc \\
  -Dotel.exporter.otlp.protocol=http/protobuf \\
  -Dotel.exporter.otlp.endpoint=${ENDPOINT}/api \\
  -Dotel.exporter.otlp.headers="authorization=Bearer ${TOKEN}" \\
  -jar app.jar`,
        },
      },
    ],
  },
  {
    id: 'node',
    category: 'languages',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      { code: { lang: 'bash', content: 'npm i @opentelemetry/api @opentelemetry/auto-instrumentations-node' } },
      {
        code: {
          lang: 'bash',
          content: `NODE_OPTIONS="--require @opentelemetry/auto-instrumentations-node/register" \\
OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf \\
OTEL_EXPORTER_OTLP_ENDPOINT=${ENDPOINT}/api \\
OTEL_EXPORTER_OTLP_HEADERS="authorization=Bearer ${TOKEN}" \\
node server.js`,
        },
      },
    ],
  },
  {
    id: 'rust',
    category: 'languages',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      {
        code: {
          lang: 'toml',
          content: `[dependencies]
opentelemetry = "0.24"
opentelemetry-otlp = { version = "0.17", features = ["http-proto", "reqwest-client"] }
tracing-opentelemetry = "0.25"
tracing-subscriber = "0.3"`,
        },
      },
      {
        code: {
          lang: 'rust',
          content: `let tracer = opentelemetry_otlp::new_pipeline()
    .tracing()
    .with_exporter(
        opentelemetry_otlp::new_exporter()
            .http()
            .with_endpoint("${ENDPOINT}/api/v1/traces")
            .with_headers(HashMap::from([(
                "authorization".to_string(),
                "Bearer ${TOKEN}".to_string(),
            )])),
    )
    .install_batch(opentelemetry_sdk::runtime::Tokio)?;`,
        },
      },
    ],
  },
  {
    id: 'dotnet',
    category: 'languages',
    signals: ['logs', 'metrics', 'traces'],
    steps: [{}],
  },
];
