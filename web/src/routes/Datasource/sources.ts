/**
 * Datasource catalogue.
 *
 * 11 top-level categories, each with a list of specific sources. Each source
 * carries a multi-step guide (description + optional code snippet) so the
 * detail pane can render them uniformly.
 *
 * Every access snippet below targets the REAL backend ingest contract
 * (`crates/api/src/http/routes/*`): native JSON on `/api/v1/ingest/{signal}/{stream}`,
 * OTLP/HTTP on `/api/v1/{logs,metrics,traces}` (there is NO OTLP gRPC :4317),
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

export type CategoryGroup = 'featured' | 'protocol' | 'source';

export type Signal = 'logs' | 'metrics' | 'traces' | 'profiles';

export interface GuideStep {
  title: string;
  description?: string;
  code?: { lang: string; content: string };
  note?: string;
}

export interface Source {
  id: string;
  name: string;
  category: Category;
  glyph: string; // short product/source mark
  glyphColor?: string;
  description: string;
  signals: Signal[];
  docsUrl?: string;
  rumPlatform?: 'browser' | 'flutter' | 'android' | 'ios';
  steps: GuideStep[];
}

export const CATEGORIES: Array<{ id: Category; label: string; group: CategoryGroup }> = [
  { id: 'recommended', label: '推荐', group: 'featured' },
  { id: 'otel', label: 'OpenTelemetry', group: 'protocol' },
  { id: 'otel-collector', label: 'OTel Collector', group: 'protocol' },
  { id: 'custom', label: '自定义', group: 'protocol' },
  { id: 'servers', label: '服务器', group: 'source' },
  { id: 'databases', label: '数据库', group: 'source' },
  { id: 'security', label: '安全', group: 'source' },
  { id: 'devops', label: 'DevOps', group: 'source' },
  { id: 'networking', label: '网络', group: 'source' },
  { id: 'queues', label: '消息队列', group: 'source' },
  { id: 'languages', label: '语言', group: 'source' },
  { id: 'ai', label: 'AI 集成', group: 'source' },
];

// Rendered placeholders. `Datasource.tsx` substitutes the live access URL,
// host / port / tls flag, and the user's default ingestion token at render
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
    name: 'Kubernetes',
    category: 'recommended',
    glyph: 'K8',
    glyphColor: '#3a8ddf',
    description: '通过 Fluent Bit DaemonSet 采集集群中所有 Pod 的 stdout/stderr 日志。',
    signals: ['logs'],
    steps: [
      {
        title: '1. 创建命名空间与 ConfigMap',
        code: {
          lang: 'bash',
          content: `kubectl create namespace telemetry
kubectl create configmap fluent-bit-config -n telemetry \\
  --from-file=fluent-bit.conf=./fluent-bit.conf`,
        },
      },
      {
        title: '2. 部署 Fluent Bit DaemonSet',
        description: '将下面 YAML 保存为 fluent-bit-daemonset.yaml，并 apply。',
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
          value: "${ENDPOINT}/api/v1/ingest/logs/default"
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
        title: '3. 验证日志已进入',
        description: '一分钟后回到 Logs 页面，stream 选 app-logs 应该能看到 kubernetes.* 字段。',
        code: { lang: 'bash', content: 'kubectl logs -n telemetry -l app=fluent-bit --tail=20' },
      },
    ],
  },
  {
    id: 'linux',
    name: 'Linux 主机',
    category: 'recommended',
    glyph: 'LX',
    description: '使用 Vector 在 Linux 主机上采集 journald / syslog。',
    signals: ['logs', 'metrics'],
    steps: [
      {
        title: '1. 安装 Vector',
        code: {
          lang: 'bash',
          content: `curl --proto '=https' --tlsv1.2 -sSf https://sh.vector.dev | bash
sudo systemctl enable --now vector`,
        },
      },
      {
        title: '2. 配置 /etc/vector/vector.yaml',
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
    uri: ${ENDPOINT}/api/v1/ingest/logs/default
    encoding: { codec: json }
    auth: { strategy: bearer, token: ${TOKEN} }`,
        },
      },
    ],
  },
  {
    id: 'windows',
    name: 'Windows 主机',
    category: 'recommended',
    glyph: 'WN',
    glyphColor: '#3a8ddf',
    description: '使用 Windows Event Forwarding + winlogbeat 推送 Event Log。',
    signals: ['logs'],
    steps: [
      {
        title: '安装 winlogbeat',
        code: {
          lang: 'powershell',
          content: `Invoke-WebRequest -Uri https://artifacts.elastic.co/.../winlogbeat-8.15.0-windows-x86_64.zip -OutFile wb.zip
Expand-Archive wb.zip -DestinationPath C:\\Program Files\\winlogbeat`,
        },
      },
      {
        title: '编辑 winlogbeat.yml',
        code: {
          lang: 'yaml',
          content: `winlogbeat.event_logs:
  - name: Application
  - name: System
  - name: Security

output.http:
  hosts: ["${ENDPOINT}/api/v1/ingest/logs/default"]
  headers:
    Authorization: "Bearer ${TOKEN}"`,
        },
      },
    ],
  },
  {
    id: 'aws',
    name: 'AWS (CloudWatch + S3)',
    category: 'recommended',
    glyph: 'AWS',
    glyphColor: '#ff8a3c',
    description: '通过 CloudWatch Logs subscription + Kinesis Firehose 把日志推送到 MoleSignal。',
    signals: ['logs', 'metrics'],
    steps: [
      {
        title: '1. 创建 Firehose delivery stream（HTTP endpoint 目标）',
        description:
          'Firehose 带不了 Bearer，走 push connector：endpoint URL 填下方地址，access key 填在「数据源 → 连接器」里创建的 connector push token。',
        code: {
          lang: 'bash',
          content: `aws firehose create-delivery-stream \\
  --delivery-stream-name molesignal-logs \\
  --http-endpoint-destination-configuration \\
    "EndpointConfiguration={Url=${ENDPOINT}/api/v1/_kinesis_firehose,AccessKey=<connector-push-token>}"`,
        },
      },
      {
        title: '2. 订阅 CloudWatch Logs',
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
    name: 'GCP (Cloud Logging)',
    category: 'recommended',
    glyph: 'GCP',
    glyphColor: '#5fc26a',
    description: '通过 Pub/Sub sink 把 Cloud Logging 推送到 MoleSignal HTTP endpoint。',
    signals: ['logs'],
    steps: [
      {
        title: '创建 Cloud Logging sink → Pub/Sub topic',
        code: {
          lang: 'bash',
          content: `gcloud logging sinks create molesignal-sink \\
  pubsub.googleapis.com/projects/$PROJECT/topics/molesignal-logs \\
  --log-filter='resource.type="k8s_container"'`,
        },
      },
      {
        title: '订阅 + 转发到 MoleSignal',
        description: `用 Cloud Run 函数读取 Pub/Sub，POST 到 ${ENDPOINT}/api/v1/ingest/logs/default，带 Authorization: Bearer ${TOKEN}。`,
      },
    ],
  },
  {
    id: 'azure',
    name: 'Azure (Event Hubs)',
    category: 'recommended',
    glyph: 'AZ',
    glyphColor: 'var(--blue)',
    description: '通过 Diagnostic Settings → Event Hub → Function 转发到 MoleSignal。',
    signals: ['logs'],
    steps: [
      {
        title: '在 Azure Portal 配置 Diagnostic Settings',
        description: `Target Event Hub，再部署 Azure Function 消费 Event Hub 并 POST 到 ${ENDPOINT}/api/v1/ingest/logs/default（带 Authorization: Bearer ${TOKEN}）。`,
      },
    ],
  },
  ...MOBILE_RUM_SOURCES,

  {
    id: 'continuous-profiling',
    name: '持续性能分析',
    category: 'recommended',
    glyph: 'PRF',
    glyphColor: '#ff8a3c',
    description: '持续性能分析（火焰图）：OTLP Profiles / Pyroscope / pprof 直传三种接入，定位 CPU / 内存 / 锁竞争。',
    signals: ['profiles'],
    steps: [
      {
        title: 'OTLP Profiles (OpenTelemetry)',
        description: '把 OTel eBPF profiler 或 SDK 指向固定提供的 OTLP profiles 端点（Alpha）。',
        code: {
          lang: 'bash',
          content: `OTEL_EXPORTER_OTLP_PROFILES_ENDPOINT=${ENDPOINT}/api/v1/profiles/otlp`,
        },
      },
      {
        title: 'Pyroscope SDK',
        description: '现有 Pyroscope agent 可直接对接兼容 ingest 端点（format=pprof|folded|lines）。',
        code: {
          lang: 'bash',
          content: `curl -X POST "${ENDPOINT}/api/v1/profiles/ingest?name=my-service&format=pprof" \\
  -H "Authorization: Bearer ${TOKEN}" \\
  --data-binary @profile.pprof`,
        },
      },
      {
        title: '原始 pprof 上传',
        description: '直接上传 gzip 压缩的 pprof（例如来自 runtime/pprof），或用 Profiles 页右上角的上传按钮。',
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
    name: 'curl（原始 JSON）',
    category: 'custom',
    glyph: '$',
    description: '最简单：HTTP POST 一个 JSON 数组到 ingest endpoint。',
    signals: ['logs'],
    steps: [
      {
        title: '发送事件',
        code: {
          lang: 'bash',
          content: `curl -X POST ${ENDPOINT}/api/v1/ingest/logs/default \\
  -H "Authorization: Bearer ${TOKEN}" \\
  -H "Content-Type: application/json" \\
  -d '[{"level":"info","service":"my-app","message":"hello MoleSignal"}]'`,
        },
      },
      {
        title: '响应',
        code: { lang: 'json', content: '{"accepted":1,"rejected":0}' },
      },
    ],
  },
  {
    id: 'bulk-ndjson',
    name: '批量 NDJSON',
    category: 'custom',
    glyph: 'NX',
    description: '批量推送：Elasticsearch 兼容 _bulk，或原生 ingest 直接收 JSON 数组。',
    signals: ['logs'],
    steps: [
      {
        title: 'Elasticsearch 兼容 _bulk',
        description: 'action + doc 行交替（NDJSON）；_index 决定目标 stream，缺省用 stream-name 头或 default。',
        code: {
          lang: 'bash',
          content: `curl -X POST ${ENDPOINT}/api/v1/_bulk \\
  -H "Authorization: Bearer ${TOKEN}" \\
  -H "Content-Type: application/x-ndjson" \\
  --data-binary @events.ndjson

# events.ndjson 内容示例：
# {"index":{"_index":"app-logs"}}
# {"level":"info","message":"a"}`,
        },
      },
      {
        title: '原生批量（JSON 数组）',
        description: '不需要 ES 格式时，直接 POST 一个 JSON 数组到 ingest endpoint，单次最多 5 MB。',
        code: {
          lang: 'bash',
          content: `curl -X POST ${ENDPOINT}/api/v1/ingest/logs/default \\
  -H "Authorization: Bearer ${TOKEN}" \\
  -H "Content-Type: application/json" \\
  -d '[{"level":"info","message":"a"},{"level":"warn","message":"b"}]'`,
        },
      },
    ],
  },
  {
    id: 'opentelemetry',
    name: 'OpenTelemetry',
    category: 'otel',
    glyph: 'OT',
    glyphColor: '#b285e0',
    description: 'OpenTelemetry SDK / 应用直连 OTLP/HTTP，logs/metrics/traces 三信号统一接入。',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      {
        title: 'OTLP/HTTP 端点',
        description:
          'endpoint 填到 /api，SDK 自动追加 /v1/{signal}（→ /api/v1/traces 等）；可选 stream-name 头选目标 stream。',
        code: {
          lang: 'text',
          content: `HTTP Endpoint: ${ENDPOINT}/api
Authorization: Bearer ${TOKEN}
stream-name:   default`,
        },
      },
      {
        title: '环境变量（SDK / auto-instrumentation）',
        code: {
          lang: 'bash',
          content: `export OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf
export OTEL_EXPORTER_OTLP_ENDPOINT=${ENDPOINT}/api
export OTEL_EXPORTER_OTLP_HEADERS="authorization=Bearer ${TOKEN}"
export OTEL_RESOURCE_ATTRIBUTES="service.name=my-svc,deployment.environment=production"`,
        },
      },
      {
        title: 'OTLP gRPC（:4317）',
        description: 'OTLP gRPC 固定监听 :4317，可通过 [otlp_grpc] 调整监听地址、端口和消息大小。',
        code: {
          lang: 'bash',
          content: `export OTEL_EXPORTER_OTLP_PROTOCOL=grpc
export OTEL_EXPORTER_OTLP_ENDPOINT=http://${ENDPOINT_HOST}:4317
export OTEL_EXPORTER_OTLP_HEADERS="authorization=Bearer ${TOKEN}"`,
        },
        note: '对外端口：生产环境建议为 :4317 配置 TLS 与防火墙策略。',
      },
    ],
  },
  {
    id: 'otel-collector',
    name: 'OTel Collector',
    category: 'otel-collector',
    glyph: 'OTC',
    glyphColor: '#b285e0',
    description: '通过 OpenTelemetry Collector 的 otlphttp exporter 中转 logs/metrics/traces 到 MoleSignal。',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      {
        title: 'collector 配置（otlphttp exporter）',
        description: 'Collector 走 OTLP/HTTP 转发：endpoint 填到 /api，SDK 自动追加 /v1/{signal}。',
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
        title: 'collector 配置（otlp gRPC exporter，:4317）',
        description: '改走固定提供的 OTLP gRPC（:4317）。明文连接加 tls.insecure: true，生产经 TLS 时去掉。',
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
  {
    id: 'syslog',
    name: 'Syslog RFC 5424',
    category: 'custom',
    glyph: 'CLI',
    description: '原生 syslog TCP/UDP 推送。',
    signals: ['logs'],
    steps: [
      {
        title: 'rsyslog → forward',
        description: '端口取决于部署的 [syslog] tcp_bind 配置，下例用常见的 6514。',
        code: {
          lang: 'text',
          content: `*.* @@${ENDPOINT_HOST}:6514;RSYSLOG_SyslogProtocol23Format`,
        },
      },
    ],
  },

  /* ───────── Servers ───────── */
  {
    id: 'nginx',
    name: 'NGINX',
    category: 'servers',
    glyph: 'N',
    glyphColor: '#5fc26a',
    description: 'access.log + error.log 通过 Fluent Bit tail。',
    signals: ['logs'],
    steps: [
      {
        title: 'Fluent Bit 输入配置',
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
    URI         /api/v1/ingest/logs/default
    Format      json
    Header      Authorization Bearer ${TOKEN}
    tls         ${ENDPOINT_TLS}`,
        },
      },
    ],
  },
  {
    id: 'apache',
    name: 'Apache httpd',
    category: 'servers',
    glyph: 'A',
    description: 'mod_log_config 标准格式 + Vector tail。',
    signals: ['logs'],
    steps: [
      { title: '参考 NGINX 示例（换用 apache parser）', description: '使用 Fluent Bit 或 Vector tail /var/log/apache2/access.log。' },
    ],
  },
  {
    id: 'haproxy',
    name: 'HAProxy',
    category: 'servers',
    glyph: 'H',
    description: '通过 stats socket + log-format JSON。',
    signals: ['logs', 'metrics'],
    steps: [
      {
        title: 'haproxy.cfg',
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
    name: 'PostgreSQL',
    category: 'databases',
    glyph: 'PG',
    description: '采集 slow query log + pg_stat_statements 指标。',
    signals: ['logs', 'metrics'],
    steps: [
      {
        title: '开启慢查询日志',
        code: {
          lang: 'sql',
          content: `ALTER SYSTEM SET log_min_duration_statement = 500;
ALTER SYSTEM SET log_statement = 'ddl';
SELECT pg_reload_conf();`,
        },
      },
      {
        title: 'Fluent Bit 采集',
        description: 'Tail /var/log/postgresql/postgresql-*.log, parser=postgres。',
      },
    ],
  },
  {
    id: 'mysql',
    name: 'MySQL',
    category: 'databases',
    glyph: 'MY',
    description: 'general_log + slow_query_log。',
    signals: ['logs', 'metrics'],
    steps: [
      {
        title: 'my.cnf',
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
    name: 'MongoDB',
    category: 'databases',
    glyph: 'MG',
    description: 'mongod 的 systemLog + Atlas DB metrics。',
    signals: ['logs', 'metrics'],
    steps: [
      {
        title: 'mongod.conf',
        code: { lang: 'yaml', content: `systemLog:\n  destination: file\n  path: /var/log/mongo/mongod.log\n  logAppend: true` },
      },
    ],
  },
  {
    id: 'clickhouse',
    name: 'ClickHouse',
    category: 'databases',
    glyph: 'CH',
    glyphColor: '#f0c14b',
    description: 'system.query_log 推送到 MoleSignal。',
    signals: ['logs', 'metrics'],
    steps: [
      {
        title: '导出 query_log（JSONEachRow）',
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
    name: 'Redis',
    category: 'databases',
    // Redis brand red (`redis.io` press kit). Vendor identity color —
    // intentionally orthogonal to the Molesignal palette.
    glyphColor: '#DC382C',
    glyph: 'R',
    description: 'redis-exporter (Prometheus) + slow log。',
    signals: ['metrics', 'logs'],
    steps: [
      {
        title: '部署 redis-exporter',
        code: { lang: 'bash', content: 'docker run -d -p 9121:9121 oliver006/redis_exporter -redis.addr=redis://localhost:6379' },
      },
      {
        title: 'Prometheus remote_write',
        description: `把 :9121/metrics 抓取后 remote_write 到 ${ENDPOINT}/api/v1/prometheus/api/v1/write（带 Authorization: Bearer ${TOKEN}）。`,
      },
    ],
  },

  /* ───────── Security ───────── */
  {
    id: 'falco',
    name: 'Falco',
    category: 'security',
    glyph: 'OSQ',
    description: 'Kubernetes 运行时安全事件。',
    signals: ['logs'],
    steps: [
      {
        title: 'falcosidekick 转发',
        description: 'Falco 原生 http_output 不能加自定义请求头，用 falcosidekick 注入 Bearer。',
        code: {
          lang: 'yaml',
          content: `webhook:\n  address: ${ENDPOINT}/api/v1/ingest/logs/security\n  customHeaders: "Authorization: Bearer ${TOKEN}"`,
        },
      },
    ],
  },
  {
    id: 'osquery',
    name: 'osquery',
    category: 'security',
    glyph: 'SQ',
    description: '主机审计 + 配置漂移。',
    signals: ['logs'],
    steps: [
      {
        title: 'osquery.conf',
        code: { lang: 'json', content: '{"options":{"logger_plugin":"http","logger_http_url":"https://...","schedule":{"users":{"query":"select * from logged_in_users;","interval":60}}}}' },
      },
    ],
  },
  {
    id: 'crowdstrike',
    name: 'CrowdStrike Falcon',
    category: 'security',
    glyph: 'CS',
    description: '通过 Falcon Streaming API。',
    signals: ['logs'],
    steps: [{ title: '使用 Falcon SIEM Connector，sink=MoleSignal HTTP', description: '需 Falcon API client+secret。' }],
  },

  /* ───────── DevOps ───────── */
  {
    id: 'github-actions',
    name: 'GitHub Actions',
    category: 'devops',
    glyph: 'GH',
    description: 'workflow_run / deployment_status events。',
    signals: ['logs'],
    steps: [
      {
        title: '工作流 webhook',
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
    name: 'ArgoCD',
    category: 'devops',
    glyph: 'TF',
    description: 'sync events + application health。',
    signals: ['logs', 'metrics'],
    steps: [{ title: 'argocd-notifications-cm', description: '配置 webhook trigger 到 MoleSignal HTTP endpoint。' }],
  },
  {
    id: 'jenkins',
    name: 'Jenkins',
    category: 'devops',
    glyph: 'J',
    description: 'build_started / build_finished events。',
    signals: ['logs'],
    steps: [{ title: '使用 Logstash plugin 或 HTTP Request plugin', description: 'POST build events 到 MoleSignal HTTP endpoint。' }],
  },

  /* ───────── Networking ───────── */
  {
    id: 'envoy',
    name: 'Envoy',
    category: 'networking',
    glyph: 'E',
    glyphColor: '#ff8a63',
    description: 'access logs JSON + statsd metrics + OTel traces。',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      {
        title: '访问日志配置',
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
    name: 'Traefik',
    category: 'networking',
    glyph: 'T',
    description: 'JSON access logs + Prometheus metrics。',
    signals: ['logs', 'metrics'],
    steps: [
      {
        title: 'traefik.yaml',
        code: { lang: 'yaml', content: `accessLog:\n  format: json\n  filePath: /var/log/traefik/access.json\nmetrics:\n  prometheus: {}` },
      },
    ],
  },
  {
    id: 'cloudflare',
    name: 'Cloudflare Logpush',
    category: 'networking',
    glyph: 'CF',
    description: 'HTTP requests logpush 到 MoleSignal endpoint。',
    signals: ['logs'],
    steps: [
      {
        title: '配置 Logpush job',
        description: `destination_conf 指向 ${ENDPOINT}/api/v1/_cloudflare，并带 header X-Connector-Token=<connector-push-token>（push connector 自鉴权，不用 Bearer）。`,
      },
    ],
  },

  /* ───────── Message Queues ───────── */
  {
    id: 'kafka',
    name: 'Apache Kafka',
    category: 'queues',
    glyph: 'K',
    glyphColor: '#64748b',
    description: '通过 Kafka Connect Sink Connector 或 kafka-exporter 推送。',
    signals: ['logs', 'metrics'],
    steps: [
      {
        title: 'Kafka Connect → HTTP Sink',
        code: {
          lang: 'json',
          content: `{
  "name": "molesignal-sink",
  "config": {
    "connector.class": "io.confluent.connect.http.HttpSinkConnector",
    "topics": "app-events",
    "http.api.url": "${ENDPOINT}/api/v1/ingest/logs/default",
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
    name: 'RabbitMQ',
    category: 'queues',
    glyph: 'RMQ',
    description: 'rabbitmq_prometheus + shovel plugin。',
    signals: ['metrics', 'logs'],
    steps: [{ title: 'Enable rabbitmq_prometheus', code: { lang: 'bash', content: 'rabbitmq-plugins enable rabbitmq_prometheus' } }],
  },
  {
    id: 'nats',
    name: 'NATS',
    category: 'queues',
    glyph: 'NA',
    description: 'NATS server monitoring endpoint /varz, /connz。',
    signals: ['metrics', 'logs'],
    steps: [{ title: 'Scrape /metrics on :7777', description: '用 Prometheus remote_write 接入。' }],
  },

  /* ───────── Languages ───────── */
  {
    id: 'python',
    name: 'Python',
    category: 'languages',
    glyph: 'PY',
    description: 'OpenTelemetry Python SDK。',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      { title: '安装', code: { lang: 'bash', content: 'pip install opentelemetry-distro opentelemetry-exporter-otlp\nopentelemetry-bootstrap -a install' } },
      {
        title: '运行',
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
    name: 'Go',
    category: 'languages',
    glyph: 'Go',
    glyphColor: 'var(--blue)',
    description: 'go.opentelemetry.io/otel SDK + otlptracehttp。',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      {
        title: 'main.go',
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
    name: 'Java',
    category: 'languages',
    glyph: 'JVM',
    description: 'OpenTelemetry Java Agent（零侵入）。',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      {
        title: '启动 JVM 时加 agent',
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
    name: 'Node.js',
    category: 'languages',
    glyph: 'N',
    glyphColor: '#5fc26a',
    description: '@opentelemetry/auto-instrumentations-node。',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      { title: '安装', code: { lang: 'bash', content: 'npm i @opentelemetry/api @opentelemetry/auto-instrumentations-node' } },
      {
        title: '启动',
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
    name: 'Rust',
    category: 'languages',
    glyph: 'RS',
    description: 'tracing-opentelemetry + opentelemetry-otlp。',
    signals: ['logs', 'metrics', 'traces'],
    steps: [
      {
        title: 'Cargo.toml',
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
        title: '初始化',
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
    name: '.NET',
    category: 'languages',
    glyph: '.N',
    description: 'OpenTelemetry.AutoInstrumentation for .NET。',
    signals: ['logs', 'metrics', 'traces'],
    steps: [{ title: '设置环境变量启动', description: '与 Java 类似，使用 OTel auto-instrumentation，OTLP_PROTOCOL=http/protobuf、ENDPOINT 指向 /api。' }],
  },

  /* ───────── AI Integrations ───────── */
  {
    id: 'openai',
    name: 'OpenAI',
    category: 'ai',
    glyph: 'AI',
    glyphColor: '#5fc26a',
    description: '采集每次 OpenAI API 调用的 prompt / completion / token 数 / 延迟。',
    signals: ['logs', 'traces'],
    steps: [
      {
        title: '使用 OpenLLMetry SDK',
        code: {
          lang: 'python',
          content: `from traceloop.sdk import Traceloop

Traceloop.init(
  app_name="my-app",
  api_endpoint="${ENDPOINT}/api",
  headers={"authorization": "Bearer ${TOKEN}"},
)

# 之后所有 openai.* 调用自动 instrument`,
        },
      },
    ],
  },
  {
    id: 'anthropic',
    name: 'Anthropic',
    category: 'ai',
    glyph: 'A',
    glyphColor: '#ff8a63',
    description: 'Claude API 调用 instrument，记录 model / prompt / output / token usage。',
    signals: ['logs', 'traces'],
    steps: [
      {
        title: 'Python',
        code: {
          lang: 'python',
          content: `from traceloop.sdk import Traceloop
Traceloop.init(app_name="my-app", api_endpoint="${ENDPOINT}/api", headers={"authorization": "Bearer ${TOKEN}"})

# anthropic.Anthropic() / .messages.create() 自动追踪`,
        },
      },
    ],
  },
  {
    id: 'langchain',
    name: 'LangChain',
    category: 'ai',
    glyph: 'LC',
    description: 'Chain / Agent step-by-step trace。',
    signals: ['logs', 'traces'],
    steps: [
      {
        title: 'callback handler',
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
    name: 'LlamaIndex',
    category: 'ai',
    glyph: 'LI',
    description: 'RAG pipeline 追踪（query → retrieve → synthesize）。',
    signals: ['traces'],
    steps: [{ title: 'set_global_handler("molesignal")', description: '类似 langchain，使用 OpenInference instrumentation。' }],
  },

  /* ───────── Custom（续：通用 / 兜底接入） ───────── */
  {
    id: 'webhook',
    name: '通用 HTTP 接入',
    category: 'custom',
    glyph: 'WH',
    description: '任意服务通过 HTTP POST JSON，把事件落到指定 stream。',
    signals: ['logs'],
    steps: [
      {
        title: '端点',
        description: '把 <stream-name> 换成目标 stream 名；不存在时由 ingestion 按需建流。',
        code: {
          lang: 'bash',
          content: `curl -X POST ${ENDPOINT}/api/v1/ingest/logs/<stream-name> \\
  -H "Authorization: Bearer ${TOKEN}" \\
  -H "Content-Type: application/json" \\
  -d '[{"event":"deploy","status":"ok"}]'`,
        },
      },
    ],
  },
  {
    id: 'graphql-subscription',
    name: 'GraphQL Subscriptions',
    category: 'custom',
    glyph: 'GQL',
    description: '订阅 GraphQL subscription 事件作为流。',
    signals: ['logs'],
    steps: [{ title: '使用 WebSocket relay', description: '部署 relay 节点，把 subscription 数据 forward 到 MoleSignal。' }],
  },
];

export function sourcesIn(cat: Category): Source[] {
  return SOURCES.filter((s) => s.category === cat);
}
