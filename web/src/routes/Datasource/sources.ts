/**
 * Datasource catalogue.
 *
 * What does not translate: each source's identifier, the signals it sends and
 * the commands of its guide. Its name, description and the words of each step
 * are in the `datasources` i18n namespace under the source's id, one entry per
 * step in the order of `steps` here (see `sourceText.ts`).
 *
 * Every access snippet below targets the REAL backend intake contract
 * (`bin/molesignal/src/api/http/routes/*`): native JSON on `/api/v1/intake/{signal}/{stream}`,
 * OTLP/HTTP on `/api/v1/{logs,metrics,traces}`, OTLP gRPC on `:4317`,
 * ES bulk on `/api/v1/_bulk`, Prometheus on `/api/v1/prometheus/api/v1/write`,
 * push connectors on `/api/v1/_kinesis_firehose|_cloudflare|_heroku`. Auth is
 * `Authorization: Bearer <token>` everywhere except push connectors (which use
 * a connector token). The org is derived server-side from the token and never
 * appears in a path.
 */

import { MOBILE_RUM_SOURCES } from './mobileRum/sources';

export type Category =
  | 'recommended'
  | 'otel'
  | 'otel-collector'
  | 'custom'
  | 'servers'
  | 'databases'
  | 'security'
  | 'devops'
  | 'networking'
  | 'queues'
  | 'languages'
  | 'ai';

export type Signal = 'logs' | 'metrics' | 'traces' | 'profiles';

export interface GuideStep {
  /** A command or file to copy; `{{ENDPOINT}}`-style slots are filled in when it is shown. */
  code?: { lang: string; content: string };
  /** A check the reader runs where the collector is deployed, shown with the verification. */
  verification?: boolean;
}

export interface Source {
  id: string;
  category: Category;
  signals: Signal[];
  docsUrl?: string;
  rumPlatform?: 'browser' | 'flutter' | 'android' | 'ios';
  steps: GuideStep[];
}

// Rendered placeholders. The guide substitutes the live access URL,
// host / port / tls flag, and the user's default intake token at render
// time (see `substitute`). They are intentionally NOT real values here so the
// catalogue stays a pure static module. Org never appears in a path.
const ENDPOINT = '{{ENDPOINT}}'; // full origin, e.g. https://obs.example.com
const ENDPOINT_HOST = '{{ENDPOINT_HOST}}'; // bare hostname, no scheme/port
const ENDPOINT_PORT = '{{ENDPOINT_PORT}}'; // numeric port
const ENDPOINT_TLS = '{{ENDPOINT_TLS}}'; // Fluent Bit tls flag: On / Off
const TOKEN = '{{TOKEN}}';

export const SOURCES: Source[] = [
  /* ───────── Recommended ───────── */
  {
    id: 'kubernetes',
    category: 'recommended',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `kubectl create namespace telemetry
kubectl create configmap fluent-bit-config -n telemetry \\
  --from-file=fluent-bit.conf=./fluent-bit.conf`,
        },
      },
      {
        code: {
          lang: 'yaml',
          content: `apiVersion: apps/v1
kind: DaemonSet
metadata:
  name: fluent-bit
  namespace: telemetry
spec:
  selector:
    matchLabels: { app: fluent-bit }
  template:
    metadata:
      labels: { app: fluent-bit }
    spec:
      serviceAccountName: fluent-bit
      containers:
      - name: fluent-bit
        image: fluent/fluent-bit:3.0
        env:
        - name: MS_ENDPOINT
          value: "${ENDPOINT}/api/v1/intake/logs/default"
        - name: MS_TOKEN
          valueFrom:
            secretKeyRef: { name: molesignal-token, key: token }
        volumeMounts:
        - { name: varlog,            mountPath: /var/log }
        - { name: varlibdockercontainers, mountPath: /var/lib/docker/containers, readOnly: true }
        - { name: config,            mountPath: /fluent-bit/etc }
      volumes:
      - { name: varlog,            hostPath: { path: /var/log } }
      - { name: varlibdockercontainers, hostPath: { path: /var/lib/docker/containers } }
      - { name: config,            configMap: { name: fluent-bit-config } }`,
        },
      },
      {
        verification: true,
        code: { lang: 'bash', content: 'kubectl logs -n telemetry -l app=fluent-bit --tail=20' },
      },
    ],
  },
  {
    id: 'linux',
    category: 'recommended',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `curl --proto '=https' --tlsv1.2 -sSf https://sh.vector.dev | bash
sudo systemctl enable --now vector`,
        },
      },
      {
        code: {
          lang: 'yaml',
          content: `sources:
  host_journald:
    type: journald
    current_boot_only: true
  host_metrics:
    type: host_metrics
    scrape_interval_secs: 15

sinks:
  molesignal_logs:
    type: http
    inputs: [host_journald]
    uri: ${ENDPOINT}/api/v1/intake/logs/default
    encoding: { codec: json }
    auth: { strategy: bearer, token: ${TOKEN} }`,
        },
      },
    ],
  },
  {
    id: 'windows',
    category: 'recommended',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'powershell',
          content: `Invoke-WebRequest -Uri https://artifacts.elastic.co/.../winlogbeat-8.15.0-windows-x86_64.zip -OutFile wb.zip
Expand-Archive wb.zip -DestinationPath C:\\Program Files\\winlogbeat`,
        },
      },
      {
        code: {
          lang: 'yaml',
          content: `winlogbeat.event_logs:
  - name: Application
  - name: System
  - name: Security

output.http:
  hosts: ["${ENDPOINT}/api/v1/intake/logs/default"]
  headers:
    Authorization: "Bearer ${TOKEN}"`,
        },
      },
    ],
  },
  {
    id: 'aws',
    category: 'recommended',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `aws firehose create-delivery-stream \\
  --delivery-stream-name molesignal-logs \\
  --http-endpoint-destination-configuration \\
    "EndpointConfiguration={Url=${ENDPOINT}/api/v1/_kinesis_firehose,AccessKey=<connector-push-token>}"`,
        },
      },
      {
        code: {
          lang: 'bash',
          content: `aws logs put-subscription-filter \\
  --log-group-name /aws/lambda/my-fn \\
  --filter-name molesignal \\
  --filter-pattern "" \\
  --destination-arn arn:aws:firehose:us-east-1:123:deliverystream/molesignal-logs`,
        },
      },
    ],
  },
  {
    id: 'gcp',
    category: 'recommended',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `gcloud logging sinks create molesignal-sink \\
  pubsub.googleapis.com/projects/$PROJECT/topics/molesignal-logs \\
  --log-filter='resource.type="k8s_container"'`,
        },
      },
      {},
    ],
  },
  {
    id: 'azure',
    category: 'recommended',
    signals: ['logs'],
    steps: [
      {},
    ],
  },
  ...MOBILE_RUM_SOURCES,

  {
    id: 'continuous-profiling',
    category: 'recommended',
    signals: ['profiles'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `OTEL_EXPORTER_OTLP_PROFILES_ENDPOINT=${ENDPOINT}/api/v1/profiles/otlp`,
        },
      },
      {
        code: {
          lang: 'bash',
          content: `curl -X POST "${ENDPOINT}/api/v1/profiles/intake?name=my-service&format=pprof" \\
  -H "Authorization: Bearer ${TOKEN}" \\
  --data-binary @profile.pprof`,
        },
      },
      {
        code: {
          lang: 'bash',
          content: `curl -X POST "${ENDPOINT}/api/v1/profiles/upload?service=my-service&type=cpu" \\
  -H "Authorization: Bearer ${TOKEN}" \\
  --data-binary @cpu.pprof`,
        },
      },
    ],
  },

  /* ───────── Custom ───────── */
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
  /* ───────── Servers ───────── */
  {
    id: 'nginx',
    category: 'servers',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'ini',
          content: `[INPUT]
    Name        tail
    Path        /var/log/nginx/access.log
    Parser      nginx
    Tag         nginx.access

[OUTPUT]
    Name        http
    Match       nginx.*
    Host        ${ENDPOINT_HOST}
    Port        ${ENDPOINT_PORT}
    URI         /api/v1/intake/logs/default
    Format      json
    Header      Authorization Bearer ${TOKEN}
    tls         ${ENDPOINT_TLS}`,
        },
      },
    ],
  },
  {
    id: 'apache',
    category: 'servers',
    signals: ['logs'],
    steps: [
      {},
    ],
  },
  {
    id: 'haproxy',
    category: 'servers',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'text',
          content: `global
  log 127.0.0.1:6514 local0
  log-format '{"client":"%ci","status":%ST,"path":"%HU","resp_ms":%Tr}'`,
        },
      },
    ],
  },

  /* ───────── Databases ───────── */
  {
    id: 'postgres',
    category: 'databases',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'sql',
          content: `ALTER SYSTEM SET log_min_duration_statement = 500;
ALTER SYSTEM SET log_statement = 'ddl';
SELECT pg_reload_conf();`,
        },
      },
      {},
    ],
  },
  {
    id: 'mysql',
    category: 'databases',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'ini',
          content: `[mysqld]
slow_query_log = 1
slow_query_log_file = /var/log/mysql/slow.log
long_query_time = 0.5`,
        },
      },
    ],
  },
  {
    id: 'mongodb',
    category: 'databases',
    signals: ['logs', 'metrics'],
    steps: [
      { code: { lang: 'yaml', content: `systemLog:\n  destination: file\n  path: /var/log/mongo/mongod.log\n  logAppend: true` } },
    ],
  },
  {
    id: 'clickhouse',
    category: 'databases',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'sql',
          content: `SELECT * FROM system.query_log
WHERE event_time >= now() - INTERVAL 1 MINUTE
FORMAT JSONEachRow`,
        },
      },
    ],
  },
  {
    id: 'redis',
    category: 'databases',
    signals: ['metrics', 'logs'],
    steps: [
      { code: { lang: 'bash', content: 'docker run -d -p 9121:9121 oliver006/redis_exporter -redis.addr=redis://localhost:6379' } },
      {},
    ],
  },

  /* ───────── Security ───────── */
  {
    id: 'falco',
    category: 'security',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'yaml',
          content: `webhook:\n  address: ${ENDPOINT}/api/v1/intake/logs/security\n  customHeaders: "Authorization: Bearer ${TOKEN}"`,
        },
      },
    ],
  },
  {
    id: 'osquery',
    category: 'security',
    signals: ['logs'],
    steps: [
      { code: { lang: 'json', content: '{"options":{"logger_plugin":"http","logger_http_url":"https://...","schedule":{"users":{"query":"select * from logged_in_users;","interval":60}}}}' } },
    ],
  },
  {
    id: 'crowdstrike',
    category: 'security',
    signals: ['logs'],
    steps: [{}],
  },

  /* ───────── DevOps ───────── */
  {
    id: 'github-actions',
    category: 'devops',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'yaml',
          content: `- name: Notify MoleSignal
  uses: peter-evans/repository-dispatch@v3
  with:
    event-type: deploy_completed
    client-payload: '{"service":"\${{ github.repository }}","sha":"\${{ github.sha }}"}'`,
        },
      },
    ],
  },
  {
    id: 'argocd',
    category: 'devops',
    signals: ['logs', 'metrics'],
    steps: [{}],
  },
  {
    id: 'jenkins',
    category: 'devops',
    signals: ['logs'],
    steps: [{}],
  },

  /* ───────── Networking ───────── */
  {
    id: 'envoy',
    category: 'networking',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      {
        code: {
          lang: 'yaml',
          content: `access_log:
- name: envoy.access_loggers.http_grpc
  typed_config:
    '@type': type.googleapis.com/envoy.extensions.access_loggers.grpc.v3.HttpGrpcAccessLogConfig
    common_config:
      log_name: envoy
      grpc_service:
        envoy_grpc: { cluster_name: molesignal }`,
        },
      },
    ],
  },
  {
    id: 'traefik',
    category: 'networking',
    signals: ['logs', 'metrics'],
    steps: [
      { code: { lang: 'yaml', content: `accessLog:\n  format: json\n  filePath: /var/log/traefik/access.json\nmetrics:\n  prometheus: {}` } },
    ],
  },
  {
    id: 'cloudflare',
    category: 'networking',
    signals: ['logs'],
    steps: [
      {},
    ],
  },

  /* ───────── Message Queues ───────── */
  {
    id: 'kafka',
    category: 'queues',
    signals: ['logs', 'metrics'],
    steps: [
      {
        code: {
          lang: 'json',
          content: `{
  "name": "molesignal-sink",
  "config": {
    "connector.class": "io.confluent.connect.http.HttpSinkConnector",
    "topics": "app-events",
    "http.api.url": "${ENDPOINT}/api/v1/intake/logs/default",
    "headers": "Authorization:Bearer ${TOKEN}",
    "request.method": "POST"
  }
}`,
        },
      },
    ],
  },
  {
    id: 'rabbitmq',
    category: 'queues',
    signals: ['metrics', 'logs'],
    steps: [{ code: { lang: 'bash', content: 'rabbitmq-plugins enable rabbitmq_prometheus' } }],
  },
  {
    id: 'nats',
    category: 'queues',
    signals: ['metrics', 'logs'],
    steps: [{}],
  },

  /* ───────── Languages ───────── */
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

  /* ───────── AI Integrations ───────── */
  {
    id: 'openai',
    category: 'ai',
    signals: ['logs', 'traces'],
    steps: [
      {
        code: {
          lang: 'python',
          content: `from traceloop.sdk import Traceloop

Traceloop.init(
  app_name="my-app",
  api_endpoint="${ENDPOINT}/api",
  headers={"authorization": "Bearer ${TOKEN}"},
)

# every openai.* call is instrumented from here on`,
        },
      },
    ],
  },
  {
    id: 'anthropic',
    category: 'ai',
    signals: ['logs', 'traces'],
    steps: [
      {
        code: {
          lang: 'python',
          content: `from traceloop.sdk import Traceloop
Traceloop.init(app_name="my-app", api_endpoint="${ENDPOINT}/api", headers={"authorization": "Bearer ${TOKEN}"})

# anthropic.Anthropic() / .messages.create() are traced automatically`,
        },
      },
    ],
  },
  {
    id: 'langchain',
    category: 'ai',
    signals: ['logs', 'traces'],
    steps: [
      {
        code: {
          lang: 'python',
          content: `from langchain_molesignal import MoleSignalCallback
chain = LLMChain(...).with_config({"callbacks": [MoleSignalCallback()]})`,
        },
      },
    ],
  },
  {
    id: 'llamaindex',
    category: 'ai',
    signals: ['traces'],
    steps: [{}],
  },

  /* ───────── Custom (continued: generic / fallback intake) ───────── */
  {
    id: 'webhook',
    category: 'custom',
    signals: ['logs'],
    steps: [
      {
        code: {
          lang: 'bash',
          content: `curl -X POST ${ENDPOINT}/api/v1/intake/logs/<stream-name> \\
  -H "Authorization: Bearer ${TOKEN}" \\
  -H "Content-Type: application/json" \\
  -d '[{"event":"deploy","status":"ok"}]'`,
        },
      },
    ],
  },
  {
    id: 'graphql-subscription',
    category: 'custom',
    signals: ['logs'],
    steps: [{}],
  },
];

export function sourcesIn(cat: Category): Source[] {
  return SOURCES.filter((s) => s.category === cat);
}
