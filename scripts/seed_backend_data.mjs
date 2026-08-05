#!/usr/bin/env node
// Seed Molesignal with representative QA data.
//
// Idempotent: streams upsert on `id`; control-plane objects are timestamped so
// re-runs append rather than collide. Telemetry rows are append-only and walk
// a sliding window relative to `Date.now()`.
//
// Usage:
//   node scripts/seed_backend_data.mjs [--skip-verify]
//   node scripts/seed_backend_data.mjs --telemetry-only
//   node scripts/seed_backend_data.mjs --topology-only
//   node scripts/seed_backend_data.mjs --rum-only
//
// Environment:
//   MS_SEED_API           API base URL                (default: http://127.0.0.1:5080/api/v1)
//   MS_SEED_EMAIL         Login email                 (default: admin@example.com)
//   MS_SEED_PASSWORD      Login password              (default: admin)
//   MS_SEED_DB            Postgres database           (default: molesignal)
//   MS_SEED_PGUSER        Postgres user               (default: molesignal)
//   MS_SEED_PGPASSWORD    Postgres password           (default: molesignal)
//   MS_SEED_PG_CONTAINER  Postgres container          (auto-detected if omitted)
//   MS_SEED_API_ONLY      Skip direct Postgres writes (schema-on-write mode)
//   MS_SEED_SKIP_VERIFY   Set to "1" to skip verify

import { execFileSync } from 'node:child_process';
import process from 'node:process';

import { buildRrwebReplay } from './rum_replay_fixture.mjs';

// ---------- Config ----------

const ARGS = new Set(process.argv.slice(2));
if (ARGS.has('-h') || ARGS.has('--help')) {
  printHelp();
  process.exit(0);
}

const API_BASE = (process.env.MS_SEED_API ?? 'http://127.0.0.1:5080/api/v1').replace(/\/$/, '');
const PG_DATABASE = process.env.MS_SEED_DB ?? 'molesignal';
const PG_USER = process.env.MS_SEED_PGUSER ?? 'molesignal';
const PG_PASSWORD = process.env.MS_SEED_PGPASSWORD ?? 'molesignal';
const LOGIN_EMAIL = process.env.MS_SEED_EMAIL ?? 'admin@example.com';
const LOGIN_PASSWORD = process.env.MS_SEED_PASSWORD ?? 'admin';
const API_ONLY = process.env.MS_SEED_API_ONLY === '1';
const SKIP_VERIFY = ARGS.has('--skip-verify') || process.env.MS_SEED_SKIP_VERIFY === '1';
const PG_CONTAINER = process.env.MS_SEED_PG_CONTAINER ?? detectPostgresContainer();

const NOW_MS = Date.now();
const NOW_US = NOW_MS * 1000;
const STAMP = new Date(NOW_MS).toISOString().replace(/[-:.TZ]/g, '').slice(0, 14);
const EXEMPLAR_MARKER_FIELD = '__molesignal_exemplar';
const EXEMPLAR_VALUE_FIELD = '__molesignal_exemplar_value';
const EXEMPLAR_LABELS_FIELD = '__molesignal_exemplar_labels';

// ---------- Helpers ----------

function printHelp() {
  console.log(`Usage:
  node scripts/seed_backend_data.mjs [--skip-verify]
  node scripts/seed_backend_data.mjs --telemetry-only
  node scripts/seed_backend_data.mjs --topology-only
  node scripts/seed_backend_data.mjs --rum-only

Seeds streams, logs, metrics, traces, RUM data, service graph edges, and
representative control-plane objects. Use --telemetry-only to append a fresh
logs/metrics/traces window without duplicating control-plane resources. Use
--rum-only to append sessions, actions, errors, Web Vitals, and replay payloads
plus their linked traces, without creating unrelated control-plane resources.

Environment:
  MS_SEED_API            API base URL. Default: ${API_BASE}
  MS_SEED_EMAIL          Login email. Default: ${LOGIN_EMAIL}
  MS_SEED_PASSWORD       Login password. Default: ${LOGIN_PASSWORD}
  MS_SEED_DB             Postgres database. Default: ${PG_DATABASE}
  MS_SEED_PGUSER         Postgres user. Default: ${PG_USER}
  MS_SEED_PGPASSWORD     Postgres password. Default: ${PG_PASSWORD}
  MS_SEED_PG_CONTAINER   Postgres container. Auto-detected when omitted.
  MS_SEED_API_ONLY       "1" to use API/schema-on-write without direct Postgres access.
  MS_SEED_SKIP_VERIFY    "1" to skip verification.`);
}

function detectPostgresContainer() {
  try {
    const rows = execFileSync('docker', ['ps', '--format', '{{.Names}}\t{{.Image}}'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    })
      .trim()
      .split('\n')
      .filter(Boolean)
      .map((line) => {
        const [name, image] = line.split('\t');
        return { name, image };
      });
    const preferred = rows.find((r) => /postgres/i.test(r.name) && /molesignal/i.test(r.name));
    if (preferred) return preferred.name;
    const legacy = rows.find((r) => /postgres/i.test(r.name) && /observabil/i.test(r.name));
    if (legacy) return legacy.name;
    const anyPg = rows.find((r) => /postgres/i.test(r.name) || /postgres/i.test(r.image));
    if (anyPg) return anyPg.name;
  } catch {
    // docker not available — fall through to historical devcontainer name.
  }
  return 'observabil_devcontainer-postgres-1';
}

const sqlString = (v) => `'${String(v).replaceAll("'", "''")}'`;
const jsonSql = (v) => `${sqlString(JSON.stringify(v))}::jsonb`;
const field = (name, dataType, indexed = false) => ({
  name,
  data_type: dataType,
  nullable: true,
  indexed,
});

function psql(sql, { capture = false } = {}) {
  const flags = ['-U', PG_USER, '-d', PG_DATABASE, '-v', 'ON_ERROR_STOP=1', '-q'];
  if (capture) flags.push('-t', '-A');
  return execFileSync(
    'docker',
    ['exec', '-i', '-e', `PGPASSWORD=${PG_PASSWORD}`, PG_CONTAINER, 'psql', ...flags],
    { input: sql, encoding: 'utf8', maxBuffer: 10 * 1024 * 1024 },
  );
}

class ApiClient {
  constructor(baseUrl) {
    this.base = baseUrl;
    this.token = null;
    this.userId = null;
    this.orgId = null;
  }

  async request(method, path, body, { auth = true } = {}) {
    const headers = { accept: 'application/json', connection: 'close' };
    if (body !== undefined) headers['content-type'] = 'application/json';
    if (auth && this.token) headers.authorization = `Bearer ${this.token}`;
    const resp = await fetch(`${this.base}${path}`, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await resp.text();
    let data = null;
    if (text) {
      try {
        data = JSON.parse(text);
      } catch {
        data = text;
      }
    }
    if (!resp.ok) {
      const msg =
        (data && typeof data === 'object' && (data.message ?? data.error)) ||
        (typeof data === 'string' && data) ||
        text;
      throw new Error(`${method} ${path} -> ${resp.status} ${msg}`);
    }
    return data;
  }

  get(path) {
    return this.request('GET', path);
  }
  post(path, body, opts) {
    return this.request('POST', path, body, opts);
  }
  put(path, body) {
    return this.request('PUT', path, body);
  }

  async login(email, password) {
    const data = await this.post('/auth/signin', { email, password }, { auth: false });
    this.#absorb(data);
  }
  async selectOrg(orgId) {
    const data = await this.post(`/orgs/${orgId}/select`, {});
    this.#absorb(data);
  }
  #absorb(data) {
    this.token = data.token ?? this.token;
    this.userId = data.user_id ?? this.userId;
    this.orgId = data.org_id ?? this.orgId;
  }
}

// ---------- Stream schemas ----------

const STREAM_SCHEMAS = {
  app_logs: {
    fields: [
      field('level', 'utf8', true),
      field('message', 'utf8', true),
      field('service', 'utf8', true),
      field('env', 'utf8', true),
      field('region', 'utf8', true),
      field('host', 'utf8', true),
      field('trace_id', 'utf8', true),
      field('span_id', 'utf8'),
      field('path', 'utf8'),
      field('method', 'utf8'),
      field('status_code', 'int64'),
      field('latency_ms', 'float64'),
      field('user_id', 'utf8'),
      field('build', 'utf8'),
      field('error', 'bool'),
    ],
  },
  http_requests_total: {
    fields: [
      field('value', 'float64'),
      field('service', 'utf8', true),
      field('route', 'utf8', true),
      field('method', 'utf8'),
      field('status', 'utf8', true),
      field('env', 'utf8'),
      field('region', 'utf8'),
      field('instance', 'utf8'),
      field(EXEMPLAR_MARKER_FIELD, 'bool'),
      field(EXEMPLAR_VALUE_FIELD, 'float64'),
      field(EXEMPLAR_LABELS_FIELD, 'json'),
    ],
  },
  http_request_duration_ms: {
    fields: [
      field('value', 'float64'),
      field('service', 'utf8', true),
      field('route', 'utf8', true),
      field('quantile', 'utf8'),
      field('env', 'utf8'),
      field('region', 'utf8'),
    ],
  },
  process_cpu_usage: {
    fields: [
      field('value', 'float64'),
      field('service', 'utf8', true),
      field('host', 'utf8', true),
      field('env', 'utf8'),
      field('region', 'utf8'),
    ],
  },
  memory_usage_mb: {
    fields: [
      field('value', 'float64'),
      field('service', 'utf8', true),
      field('host', 'utf8', true),
      field('env', 'utf8'),
      field('region', 'utf8'),
    ],
  },
  traces: {
    fields: [
      field('trace_id', 'utf8', true),
      field('span_id', 'utf8', true),
      field('parent_span_id', 'utf8'),
      field('service_name', 'utf8', true),
      field('operation_name', 'utf8', true),
      field('start_time_unix_nano', 'int64'),
      field('end_time_unix_nano', 'int64'),
      field('status_code', 'utf8'),
      field('duration_us', 'int64'),
      field('http_method', 'utf8'),
      field('http_route', 'utf8'),
      field('http_status_code', 'int64'),
      field('peer_service', 'utf8'),
      field('attributes', 'json'),
      field('events', 'json'),
    ],
  },
  topology_traces: {
    fields: [
      field('trace_id', 'utf8', true),
      field('span_id', 'utf8', true),
      field('parent_span_id', 'utf8'),
      field('service.name', 'utf8', true),
      field('name', 'utf8', true),
      field('kind', 'int64'),
      field('start_time_unix_nano', 'int64'),
      field('end_time_unix_nano', 'int64'),
      field('duration_ns', 'int64'),
      field('status_code', 'utf8'),
      field('http.method', 'utf8'),
      field('http.route', 'utf8'),
      field('http.status_code', 'int64'),
    ],
  },
  rum_sessions: {
    fields: [
      field('session_id', 'utf8', true),
      field('user_id', 'utf8'),
      field('ip_address', 'utf8'),
      field('country', 'utf8'),
      field('browser', 'utf8'),
      field('application', 'utf8'),
      field('environment', 'utf8'),
      field('version', 'utf8'),
      field('device', 'utf8'),
      field('os', 'utf8'),
      field('last_page', 'utf8'),
      field('duration_ms', 'float64'),
      field('error_count', 'int64'),
      field('started_at_micros', 'int64'),
      field('timestamp', 'int64'),
    ],
  },
  rum_actions: {
    fields: [
      field('session_id', 'utf8', true),
      field('ts_micros', 'int64'),
      field('type', 'utf8', true),
      field('name', 'utf8'),
      field('page', 'utf8'),
      field('application', 'utf8'),
      field('environment', 'utf8'),
      field('version', 'utf8'),
      field('country', 'utf8'),
      field('browser', 'utf8'),
      field('device', 'utf8'),
      field('os', 'utf8'),
      field('url', 'utf8'),
      field('duration_ms', 'float64'),
      field('status', 'int64'),
      field('lcp_ms', 'float64'),
      field('fid_ms', 'float64'),
      field('cls', 'float64'),
      field('ttfb_ms', 'float64'),
      field('payload', 'json'),
      field('timestamp', 'int64'),
      // RUM ↔ backend trace 跨信号 handle。
      // RUM SDK fetch/XHR interceptor 解 W3C traceparent 后写入这 3 列。
      field('service', 'utf8', true),
      field('trace_id', 'utf8', true),
      field('parent_span_id', 'utf8'),
    ],
  },
  rum_errors: {
    fields: [
      field('session_id', 'utf8', true),
      field('user_id', 'utf8'),
      field('fingerprint', 'utf8', true),
      field('message', 'utf8', true),
      field('application', 'utf8'),
      field('environment', 'utf8'),
      field('version', 'utf8'),
      field('page', 'utf8'),
      field('error_type', 'utf8'),
      field('error', 'json'),
      field('timestamp', 'int64'),
    ],
  },
  app_logs_enriched: {
    fields: [
      field('level', 'utf8'),
      field('message', 'utf8'),
      field('service', 'utf8'),
      field('trace_id', 'utf8'),
      field('customer_tier', 'utf8'),
    ],
  },
};

STREAM_SCHEMAS.http_requests_total_5m = {
  fields: [
    ...STREAM_SCHEMAS.http_requests_total.fields,
    field('window', 'utf8'),
    field('rollup', 'utf8'),
  ],
};
STREAM_SCHEMAS.traces_enriched = {
  fields: [...STREAM_SCHEMAS.traces.fields],
};

// (streamName, streamType) — id 与 name 一致，避免前端误把 id 当 name 时报
// "stream not found"。代价是 streams.id 跨 org 不再独立，dev 单 org 场景足够。
const STREAM_DEFS = [
  ['app_logs', 'logs'],
  ['http_requests_total', 'metrics'],
  ['http_request_duration_ms', 'metrics'],
  ['process_cpu_usage', 'metrics'],
  ['memory_usage_mb', 'metrics'],
  ['http_requests_total_5m', 'metrics'],
  ['traces', 'traces'],
  ['topology_traces', 'traces'],
  ['traces_enriched', 'traces'],
  ['rum_sessions', 'logs'],
  ['rum_actions', 'logs'],
  ['rum_errors', 'logs'],
  ['app_logs_enriched', 'logs'],
];

// ---------- Seed: streams ----------

function seedStreams(orgId) {
  // 旧版本脚本用 `seed-stream-<name>` 当 id，跟 (org_id, name, stream_type) unique
  // 索引强绑死。新版用 id = name，先把残留行删掉，否则唯一索引会撞。
  psql(`
    DELETE FROM streams
     WHERE org_id = ${sqlString(orgId)}
       AND id LIKE 'seed-stream-%';
  `);

  const rows = STREAM_DEFS.map(([name, st]) => {
    const schema = STREAM_SCHEMAS[name];
    return `(
      ${sqlString(name)}, ${sqlString(orgId)},
      ${sqlString(name)}, ${sqlString(st)},
      ${jsonSql(schema)}, ${jsonSql({ days: 30 })},
      ${NOW_US}, ${NOW_US}
    )`;
  });
  psql(`
    INSERT INTO streams
      (id, org_id, name, stream_type, schema, retention, created_at_micros, updated_at_micros)
    VALUES ${rows.join(',\n')}
    ON CONFLICT (id) DO UPDATE
      SET schema = EXCLUDED.schema,
          retention = EXCLUDED.retention,
          org_id = EXCLUDED.org_id,
          name = EXCLUDED.name,
          stream_type = EXCLUDED.stream_type,
          updated_at_micros = EXCLUDED.updated_at_micros;
  `);
  return `streams: upserted ${rows.length} (${STREAM_DEFS.map(([n]) => n).join(', ')})`;
}

function resetSeedParquetFileMeta(orgId) {
  const names = STREAM_DEFS.map(([n]) => sqlString(n)).join(', ');
  psql(`
    UPDATE parquet_file_meta
       SET deleted = TRUE
     WHERE org_id = ${sqlString(orgId)}
       AND stream IN (${names});
  `);
}

// ---------- Data generators ----------

const traceId = (idx) => `trace_seed_${STAMP}_${idx}`;

function makeLogs() {
  const services = ['gateway', 'checkout', 'payments', 'inventory'];
  const routes = ['/api/checkout', '/api/payments', '/api/cart', '/api/inventory'];
  return Array.from({ length: 80 }, (_, i) => {
    const service = services[i % services.length];
    const isErr = i % 13 === 0;
    const route = routes[i % routes.length];
    const ts = NOW_US - (80 - i) * 30_000_000;
    return {
      _timestamp: ts,
      level: isErr ? 'error' : i % 5 === 0 ? 'warn' : 'info',
      message: isErr
        ? `${service} failed to process ${route} request`
        : `${service} handled ${route} request`,
      service,
      env: 'prod',
      region: i % 2 === 0 ? 'us-east-1' : 'us-west-2',
      host: `ip-10-0-${Math.floor(i / 10)}-${10 + (i % 10)}`,
      trace_id: traceId(i % 6),
      span_id: `span_${i % 6}_${i % 4}`,
      path: route,
      method: i % 3 === 0 ? 'POST' : 'GET',
      status_code: isErr ? 500 : i % 7 === 0 ? 429 : 200,
      latency_ms: Number((45 + (i % 9) * 18 + (isErr ? 240 : 0)).toFixed(2)),
      user_id: `user-${1000 + (i % 12)}`,
      build: `2026.05.${20 + (i % 7)}`,
      error: isErr,
    };
  });
}

function makeEnrichedLogs(logs) {
  const tiers = ['pro', 'team', 'growth', 'oss'];
  return logs.map((row, i) => ({
    _timestamp: row._timestamp,
    level: String(row.level).toUpperCase(),
    message: `${row.message} · enriched`,
    service: row.service,
    trace_id: row.trace_id,
    customer_tier: tiers[i % tiers.length],
  }));
}

function counterSeries(metric, service, route, status, startValue) {
  const pointCount = 30;
  return Array.from({ length: pointCount }, (_, i) => ({
    _timestamp: NOW_US - (pointCount - 1 - i) * 120_000_000,
    value: startValue + i * (status === '500' ? 3 : 42) + (i % 3),
    service,
    route,
    method: route.includes('checkout') ? 'POST' : 'GET',
    status,
    env: 'prod',
    region: service === 'gateway' ? 'us-east-1' : 'us-west-2',
    instance: `${metric}-${service}-${status}`,
  }));
}

function gaugeSeries(service, host, baseValue, jitter) {
  const pointCount = 20;
  return Array.from({ length: pointCount }, (_, i) => ({
    _timestamp: NOW_US - (pointCount - 1 - i) * 180_000_000,
    value: Number((baseValue + ((i * 7) % 11) * jitter).toFixed(3)),
    service,
    host,
    env: 'prod',
    region: i % 2 === 0 ? 'us-east-1' : 'us-west-2',
  }));
}

function makeRequestExemplars() {
  // counterSeries 覆盖过去 58 分钟；这 6 个 trace 位于过去 24..4 分钟，
  // 正好落在同一条 gateway series 上。错误 trace 指向 payment 的失败 span。
  return Array.from({ length: 6 }, (_, traceIndex) => {
    const failed = traceIndex === 2;
    const status = failed ? '500' : '200';
    const sampleIndex = 17 + traceIndex * 2;
    const startValue = failed ? 120 : 10_000;
    const increment = failed ? 3 : 42;
    return {
      _timestamp: NOW_US - (6 - traceIndex) * 240_000_000 + (failed ? 16_000 : 0),
      service: 'gateway',
      route: '/api/checkout',
      method: 'POST',
      status,
      env: 'prod',
      region: 'us-east-1',
      instance: `http_requests_total-gateway-${status}`,
      [EXEMPLAR_MARKER_FIELD]: true,
      [EXEMPLAR_VALUE_FIELD]: startValue + sampleIndex * increment + (sampleIndex % 3),
      [EXEMPLAR_LABELS_FIELD]: {
        trace_id: traceId(traceIndex),
        span_id: `span_${traceIndex}_${failed ? 2 : 0}`,
      },
    };
  });
}

function makeTraces() {
  const services = ['gateway', 'checkout', 'payments', 'inventory'];
  const rows = [];
  for (let t = 0; t < 6; t += 1) {
    const trace = traceId(t);
    const startUs = NOW_US - (6 - t) * 240_000_000;
    for (let s = 0; s < 4; s += 1) {
      const durationUs = 20_000 + t * 4_000 + s * 15_000;
      const spanStartUs = startUs + s * 8_000;
      const spanEndUs = spanStartUs + durationUs;
      const isErr = t === 2 && s === 2;
      rows.push({
        _timestamp: spanStartUs,
        trace_id: trace,
        span_id: `span_${t}_${s}`,
        parent_span_id: s === 0 ? '' : `span_${t}_${s - 1}`,
        service_name: services[s],
        operation_name: s === 0 ? 'HTTP POST /api/checkout' : `${services[s]}.call`,
        start_time_unix_nano: spanStartUs * 1000,
        end_time_unix_nano: spanEndUs * 1000,
        status_code: isErr ? 'ERROR' : 'OK',
        duration_us: durationUs,
        http_method: s === 0 ? 'POST' : 'GET',
        http_route: s === 0 ? '/api/checkout' : `/internal/${services[s]}`,
        http_status_code: isErr ? 502 : 200,
        peer_service: services[s + 1] ?? '',
        attributes: {
          env: 'prod',
          region: t % 2 === 0 ? 'us-east-1' : 'us-west-2',
          customer_tier: t % 2 === 0 ? 'pro' : 'team',
        },
        events: isErr
          ? [
              {
                ts_ns: spanEndUs * 1000,
                name: 'exception',
                attributes: { message: 'payment provider timeout' },
              },
            ]
          : [],
      });
    }
  }
  return rows;
}

function makeTopologyTraces() {
  const spans = [
    { service: 'api-gateway', parent: null, operation: 'POST /api/checkout', durationMs: 184 },
    { service: 'auth-service', parent: 0, operation: 'auth.verify', durationMs: 18 },
    { service: 'checkout-service', parent: 0, operation: 'checkout.create', durationMs: 151 },
    { service: 'payment-service', parent: 2, operation: 'payment.authorize', durationMs: 93 },
    { service: 'inventory-service', parent: 2, operation: 'inventory.reserve', durationMs: 41 },
    { service: 'bank-gateway', parent: 3, operation: 'POST /v2/charges', durationMs: 67 },
  ];
  const rows = [];
  for (let t = 0; t < 18; t += 1) {
    const trace = `${STAMP}${String(t).padStart(18, '0')}`.slice(0, 32);
    const baseUs = NOW_US - (t + 3) * 60_000_000;
    for (let s = 0; s < spans.length; s += 1) {
      const spec = spans[s];
      const spanId = `${String(t).padStart(8, '0')}${String(s).padStart(8, '0')}`;
      const parentSpanId =
        spec.parent == null
          ? ''
          : `${String(t).padStart(8, '0')}${String(spec.parent).padStart(8, '0')}`;
      const startUs = baseUs + s * 2_000;
      const durationNs = (spec.durationMs + ((t * 11 + s * 7) % 29)) * 1_000_000;
      const isError = (t % 7 === 0 && s === 3) || (t % 11 === 0 && s === 5);
      rows.push({
        _timestamp: startUs,
        trace_id: trace,
        span_id: spanId,
        parent_span_id: parentSpanId,
        'service.name': spec.service,
        name: spec.operation,
        kind: s === 0 ? 2 : 3,
        start_time_unix_nano: startUs * 1000,
        end_time_unix_nano: startUs * 1000 + durationNs,
        duration_ns: durationNs,
        status_code: isError ? 'ERROR' : 'OK',
        'http.method': s === 0 || s === 5 ? 'POST' : 'INTERNAL',
        'http.route': spec.operation,
        'http.status_code': isError ? 502 : 200,
      });
    }
  }
  return rows;
}

// ---------- Seed: telemetry ----------

async function seedTelemetry(api) {
  const logs = makeLogs();
  const enriched = makeEnrichedLogs(logs);
  const traces = makeTraces();
  const topologyTraces = makeTopologyTraces();

  const requestMetrics = [
    ...counterSeries('http_requests_total', 'gateway', '/api/checkout', '200', 10_000),
    ...counterSeries('http_requests_total', 'gateway', '/api/checkout', '500', 120),
    ...counterSeries('http_requests_total', 'checkout', '/api/payments', '200', 8_500),
  ];
  const requestExemplars = makeRequestExemplars();
  const durations = [
    ...gaugeSeries('gateway', 'gw-1', 85, 6).map((x) => ({ ...x, route: '/api/checkout', quantile: 'p95' })),
    ...gaugeSeries('checkout', 'checkout-1', 145, 9).map((x) => ({ ...x, route: '/api/payments', quantile: 'p95' })),
  ];
  const cpu = [
    ...gaugeSeries('gateway', 'gw-1', 0.42, 0.018),
    ...gaugeSeries('checkout', 'checkout-1', 0.55, 0.021),
    ...gaugeSeries('payments', 'payments-1', 0.63, 0.016),
  ];
  const memory = [
    ...gaugeSeries('gateway', 'gw-1', 512, 7),
    ...gaugeSeries('checkout', 'checkout-1', 724, 9),
    ...gaugeSeries('payments', 'payments-1', 834, 11),
  ];

  await api.post('/ingest/logs/app_logs', logs);
  await api.post('/ingest/logs/app_logs_enriched', enriched);
  await api.post('/ingest/metrics/http_requests_total', [
    ...requestMetrics,
    ...requestExemplars,
  ]);
  await api.post('/ingest/metrics/http_request_duration_ms', durations);
  await api.post('/ingest/metrics/process_cpu_usage', cpu);
  await api.post('/ingest/metrics/memory_usage_mb', memory);
  await api.post(
    '/ingest/metrics/http_requests_total_5m',
    requestMetrics.slice(-12).map((row) => ({ ...row, window: '5m', rollup: 'rate' })),
  );
  await api.post('/ingest/traces/traces', traces);
  await api.post('/ingest/traces/topology_traces', topologyTraces);
  await api.post(
    '/ingest/traces/traces_enriched',
    traces.slice(0, 12).map((row) => ({
      ...row,
      attributes: {
        ...(row.attributes ?? {}),
        seeded: true,
        owner: 'platform',
      },
    })),
  );

  return [
    `logs: ${logs.length} into app_logs, ${enriched.length} into app_logs_enriched`,
    `metrics: ${requestMetrics.length + requestExemplars.length + durations.length + cpu.length + memory.length + 12} rows across 5 streams (${requestExemplars.length} exemplars)`,
    `traces: ${traces.length + topologyTraces.length + 12} spans across 25 traces`,
  ];
}

// ---------- Seed: profiles ----------

async function seedProfiles(api) {
  const profiles = Array.from({ length: 8 }, (_, i) => {
    const start = NOW_US - (i + 1) * 300_000_000;
    const checkoutWeight = 680 + i * 37;
    const databaseWeight = 410 + ((i * 53) % 190);
    const paymentsWeight = 280 + ((i * 41) % 160);
    const gcWeight = 90 + ((i * 17) % 70);
    return {
      start,
      until: start + 60_000_000,
      name: 'checkout-api.cpu{env=production,region=us-east-1,version=2026.07.25}',
      body: [
        `runtime.main;net/http.(*Server).Serve;checkout.(*Handler).CreateOrder ${checkoutWeight}`,
        `runtime.main;net/http.(*Server).Serve;checkout.(*Handler).CreateOrder;orders.(*Repository).Save;database/sql.(*DB).exec ${databaseWeight}`,
        `runtime.main;net/http.(*Server).Serve;checkout.(*Handler).CreateOrder;payments.(*Client).Authorize;crypto/tls.(*Conn).Read ${paymentsWeight}`,
        `runtime.main;net/http.(*Server).Serve;checkout.(*Handler).CreateOrder;inventory.(*Client).Reserve;encoding/json.Marshal ${220 + i * 19}`,
        `runtime.main;runtime.gcBgMarkWorker;runtime.scanobject ${gcWeight}`,
        `runtime.main;go.opentelemetry.io/otel/sdk/trace.(*batchSpanProcessor).processQueue ${120 + i * 11}`,
      ].join('\n'),
    };
  });

  for (const profile of profiles) {
    const query = new URLSearchParams({
      name: profile.name,
      format: 'folded',
      from: String(profile.start),
      until: String(profile.until),
    });
    const resp = await fetch(`${api.base}/profiles/ingest?${query}`, {
      method: 'POST',
      headers: {
        authorization: `Bearer ${api.token}`,
        'content-type': 'text/plain; charset=utf-8',
      },
      body: profile.body,
    });
    if (!resp.ok) {
      throw new Error(`POST /profiles/ingest -> ${resp.status} ${await resp.text()}`);
    }
  }

  return [`profiles: ${profiles.length} folded CPU profiles for checkout-api`];
}

// ---------- Seed: RUM ----------

async function seedRum(api) {
  const journeys = [
    ['/', '/products', '/checkout'],
    ['/', '/search', '/product/sku-104'],
    ['/campaign/summer', '/product/sku-201', '/checkout'],
    ['/', '/account/orders'],
    ['/', '/products', '/product/sku-316', '/checkout'],
    ['/search', '/product/sku-104'],
  ];
  const browsers = ['Chrome', 'Safari', 'Chrome', 'Edge'];
  const countries = ['US', 'DE', 'BR', 'JP'];
  const ipAddresses = [
    '203.0.113.42',
    '198.51.100.18',
    '192.0.2.73',
    '203.0.113.91',
  ];
  const versions = ['v2.4.1', 'v2.4.1', 'v2.4.0', 'v2.3.9'];
  const sessions = Array.from({ length: 12 }, (_, i) => {
    const started = NOW_US - (i + 1) * 210_000_000;
    const journey = journeys[i % journeys.length];
    const errorCount = i === 2 || i === 6 || i === 10 ? (i === 2 ? 2 : 1) : 0;
    return {
      timestamp: started,
      session_id: `rum_seed_${STAMP}_${i}`,
      user_id: `user-${200 + i}`,
      ip_address: ipAddresses[i % ipAddresses.length],
      country: countries[i % countries.length],
      browser: browsers[i % browsers.length],
      application: 'checkout-web',
      environment: i === 11 ? 'staging' : 'production',
      version: versions[i % versions.length],
      device: i % 5 === 0 ? 'mobile' : 'desktop',
      os: i % 5 === 0 ? 'iOS' : i % 2 === 0 ? 'macOS' : 'Windows',
      last_page: journey[journey.length - 1],
      duration_ms: 92_000 + i * 14_000,
      error_count: errorCount,
      started_at_micros: started,
    };
  });
  const actions = sessions.flatMap((session, i) => {
    const journey = journeys[i % journeys.length];
    const common = {
      application: session.application,
      environment: session.environment,
      version: session.version,
      country: session.country,
      browser: session.browser,
      device: session.device,
      os: session.os,
    };
    const views = journey.map((page, pageIndex) => {
      const checkoutPenalty = page === '/checkout' ? 1_100 : 0;
      const safariPenalty = session.browser === 'Safari' ? 650 : 0;
      const badReleasePenalty = session.version === 'v2.4.1' && i % 3 === 2 ? 1_800 : 0;
      return {
        ...common,
        timestamp: session.started_at_micros + (pageIndex * 12 + 1) * 1_000_000,
        session_id: session.session_id,
        ts_micros: session.started_at_micros + (pageIndex * 12 + 1) * 1_000_000,
        type: 'view',
        name: `View ${page}`,
        page,
        lcp_ms: 1_350 + i * 80 + checkoutPenalty + safariPenalty + badReleasePenalty,
        fid_ms: 28 + i * 3 + (session.device === 'mobile' ? 68 : 0),
        cls: Number((0.018 + i * 0.006 + (page === '/checkout' ? 0.025 : 0)).toFixed(3)),
        ttfb_ms: 110 + i * 22 + (session.country === 'BR' ? 480 : 0),
        payload: {
          path: page,
          application: session.application,
          environment: session.environment,
          version: session.version,
          browser: session.browser,
          country: session.country,
          device: session.device,
        },
        service: 'gateway',
      };
    });
    const finalPage = journey[journey.length - 1];
    const interactionType =
      i === 1 || i === 5 ? 'rage_click' : i === 7 ? 'dead_click' : i === 10 ? 'crash' : 'click';
    const interaction = {
      ...common,
      timestamp: session.started_at_micros + 48_000_000,
      session_id: session.session_id,
      ts_micros: session.started_at_micros + 48_000_000,
      type: interactionType,
      name:
        interactionType === 'rage_click'
          ? 'Repeated payment button click'
          : interactionType === 'dead_click'
            ? 'Unresponsive coupon control'
            : interactionType === 'crash'
              ? 'Checkout page crashed'
              : 'Continue checkout',
      page: finalPage,
      payload: {
        path: finalPage,
        selector:
          interactionType === 'dead_click' ? '#coupon-toggle' : '#checkout-primary-action',
      },
      service: 'gateway',
    };
    const failed = i === 2 || i === 6 || i === 10;
    const resource = {
      ...common,
      timestamp: session.started_at_micros + 52_000_000,
      session_id: session.session_id,
      ts_micros: session.started_at_micros + 52_000_000,
      type: 'resource',
      name: 'POST /api/checkout',
      page: finalPage,
      url: '/api/checkout',
      duration_ms: i === 4 || i === 8 ? 1_450 + i * 90 : 180 + i * 35,
      status: failed ? (i === 6 ? 504 : 500) : 200,
      payload: { method: 'POST', path: finalPage },
      // 关联到 makeTraces 生成的 6 条 trace（trace_seed_<stamp>_0..5）。
      service: 'gateway',
      trace_id: traceId(i % 6),
      parent_span_id: `span_${i % 6}_0`,
    };
    return [...views, interaction, resource];
  });
  const errors = [
    {
      timestamp: NOW_US - 300_000_000,
      session_id: sessions[2].session_id,
      user_id: sessions[2].user_id,
      application: sessions[2].application,
      environment: sessions[2].environment,
      version: sessions[2].version,
      page: '/checkout',
      error_type: 'NetworkError',
      fingerprint: `rum-error-${STAMP}-payment-timeout`,
      message: 'Payment provider timeout',
      error: {
        stack: [
          { file: 'checkout.js', function: 'submitPayment', line: 42, column: 17 },
          { file: 'api.js', function: 'post', line: 12, column: 3 },
        ],
      },
    },
    {
      timestamp: NOW_US - 720_000_000,
      session_id: sessions[6].session_id,
      user_id: sessions[6].user_id,
      application: sessions[6].application,
      environment: sessions[6].environment,
      version: sessions[6].version,
      page: '/checkout',
      error_type: 'GatewayTimeout',
      fingerprint: `rum-error-${STAMP}-payment-timeout`,
      message: 'Payment provider timeout',
      error: {
        stack: [
          { file: 'checkout.js', function: 'submitPayment', line: 42, column: 17 },
          { file: 'api.js', function: 'post', line: 12, column: 3 },
        ],
      },
    },
    {
      timestamp: NOW_US - 1_020_000_000,
      session_id: sessions[10].session_id,
      user_id: sessions[10].user_id,
      application: sessions[10].application,
      environment: sessions[10].environment,
      version: sessions[10].version,
      page: sessions[10].last_page,
      error_type: 'TypeError',
      fingerprint: `rum-error-${STAMP}-undefined-total`,
      message: 'TypeError: cannot read total of undefined',
      error: {
        stack: [
          { file: 'cart.js', function: 'calculateTotal', line: 86, column: 9 },
          { file: 'checkout.js', function: 'renderSummary', line: 28, column: 4 },
        ],
      },
    },
  ];

  await api.post('/rum/sessions', sessions);
  await api.post('/rum/actions', actions);
  await api.post('/rum/errors', errors);
  for (const [i, session] of sessions.entries()) {
    const journey = journeys[i % journeys.length];
    const replayStart = Math.floor(session.started_at_micros / 1_000);
    const interaction =
      i === 1 || i === 5
        ? 'rage_click'
        : i === 7
          ? 'dead_click'
          : i === 10
            ? 'crash'
            : 'click';
    const events = buildRrwebReplay({
      startMs: replayStart,
      origin: 'https://checkout.example.com',
      journey,
      interaction,
      errorLabel:
        session.error_count > 0
          ? i === 6
            ? 'POST /api/checkout · 504'
            : 'POST /api/checkout · 500'
          : undefined,
      viewport:
        session.device === 'mobile'
          ? { width: 390, height: 844 }
          : { width: 1440, height: 900 },
    });
    await api.post('/rum/replay', {
      session_id: session.session_id,
      seq: 1,
      events,
    });
  }

  return [`rum: ${sessions.length} sessions, ${actions.length} actions, ${errors.length} errors, ${sessions.length} replay payloads`];
}

// ---------- Seed: service graph ----------

function seedServiceGraph(orgId) {
  const bucket = Math.floor(NOW_US / 60_000_000) * 60_000_000;
  const edges = [
    ['gateway', 'checkout', 640, 6, 42_000, 88_000, 134_000],
    ['checkout', 'payments', 610, 18, 55_000, 145_000, 260_000],
    ['checkout', 'inventory', 490, 2, 30_000, 74_000, 110_000],
    ['payments', 'bank-gateway', 230, 14, 95_000, 220_000, 430_000],
  ];
  const values = edges.map(([client, server, count, errs, p50, p95, p99], i) => `(
    ${sqlString(`seed-sg-${STAMP}-${i}`)}, ${sqlString(orgId)},
    ${sqlString(client)}, ${sqlString(server)},
    ${bucket - i * 60_000_000}, ${count}, ${errs}, ${p50}, ${p95}, ${p99}
  )`);
  psql(`
    INSERT INTO service_graph_edges
      (id, org_id, client_service, server_service, bucket_at_micros,
       request_count, error_count, p50_us, p95_us, p99_us)
    VALUES ${values.join(',\n')}
    ON CONFLICT (id) DO NOTHING;
  `);
  return `service_graph_edges: ${edges.length} edges`;
}

// ---------- Seed: control plane ----------

async function seedControlPlane(api, { directDb = true } = {}) {
  const escalation = await api.post('/alerts/escalations', {
    name: `seed-escalation-${STAMP}`,
    steps: [
      {
        targets: [{ kind: 'user', user_id: api.userId }],
        ack_timeout_secs: 180,
      },
    ],
    repeat: true,
    max_loops: 2,
  });
  await api.post('/alerts/rules', {
    name: `seed checkout error rate ${STAMP}`,
    description: 'Seeded alert rule for QA data.',
    enabled: true,
    query: {
      language: 'sql',
      statement: "SELECT COUNT(*) FROM http_requests_total WHERE status = '500'",
      period_secs: 300,
      stream: { name: 'http_requests_total', stream_type: 'metrics' },
    },
    trigger: { operator: 'gt', threshold: 1, for_periods: 1, silence_secs: 300 },
    escalation_policy_id: escalation.id,
    labels: { severity: 'warning', service: 'checkout' },
    annotations: { runbook: 'https://example.com/runbooks/checkout-errors' },
  });

  const schedule = await api.post('/schedules', {
    name: `seed primary on-call ${STAMP}`,
    description: 'Production core alert escalation routing.',
    timezone: 'UTC',
    enabled: true,
    rotations: [
      {
        id: `seed-rotation-${STAMP}`,
        name: 'primary',
        members: [api.userId],
        kind: 'daily',
        start_at: NOW_US - 86_400_000_000,
      },
    ],
    overrides: [],
  });
  await api.post(`/schedules/${schedule.id}/overrides`, {
    user_id: api.userId,
    start_at_micros: NOW_US - 600_000_000,
    end_at_micros: NOW_US + 600_000_000,
    reason: 'Seeded QA override',
  });

  const notifyTeam = await api.post('/teams', {
    name: `seed notify responders ${STAMP}`,
    member_ids: [api.userId],
  });
  const notifyConnector = await api.post('/notify/connectors', {
    name: `seed notify SMTP ${STAMP}`,
    connector_type: 'email_smtp',
    config: {
      host: '127.0.0.1',
      port: 2525,
      username: '',
      password: `seed-notify-secret-${STAMP}`,
      from: 'molesignal@example.com',
      tls: 'none',
      timeout_secs: 1,
    },
    enabled: true,
  });
  const notifyEndpoint = await api.post(`/users/${api.userId}/notify-endpoints`, {
    connector_id: notifyConnector.id,
    external_identity: `seed-owner-${STAMP}@example.com`,
    display_name: 'Seed owner email',
    metadata: { source: 'seed_backend_data' },
    verified: true,
    enabled: true,
  });
  await api.put(`/users/${api.userId}/notify-preferences/alert`, {
    enabled: true,
    endpoint_ids: [notifyEndpoint.id],
    quiet_hours: null,
    allow_critical_bypass: true,
  });
  await api.put(`/notify/team-defaults/${notifyTeam.id}/alert`, {
    enabled: true,
    routes: [
      {
        connector_id: notifyConnector.id,
        target_type: 'fixed_address',
        target: `seed-team-${STAMP}@example.com`,
        order: 1,
      },
    ],
  });
  await api.put('/notify/organization-defaults/alert', {
    enabled: true,
    routes: [
      {
        connector_id: notifyConnector.id,
        target_type: 'fixed_address',
        target: `seed-org-${STAMP}@example.com`,
        order: 1,
      },
    ],
  });
  const notifyTemplate = await api.post('/notify/templates', {
    name: `seed notify event ${STAMP}`,
    body:
      '[{{severity}}] {{summary}}\nService: {{labels.service}}\nEvent: {{event.id}}',
    format: 'markdown',
  });
  await api.post('/notify/policies', {
    name: `seed alert routing ${STAMP}`,
    event_type: 'alert.triggered',
    category: 'alert',
    matchers: { severity: 'warning' },
    recipient_resolver: 'fixed_users',
    resolver_config: {
      user_ids: [api.userId],
      team_id: notifyTeam.id,
    },
    delivery_mode: 'prefer_user',
    delivery_config: { connector_ids: [] },
    template_id: notifyTemplate.id,
    fallback_config: {
      use_user_fallbacks: true,
      use_team_defaults: true,
      use_organization_defaults: true,
    },
    ack_timeout_seconds: 300,
    escalation_config: {
      recipient_resolver: 'current_oncall',
      resolver_config: { schedule_id: schedule.id },
      delivery_mode: 'prefer_user',
      delivery_config: { connector_ids: [] },
      fallback_config: {
        use_user_fallbacks: true,
        use_team_defaults: true,
        use_organization_defaults: true,
      },
    },
    enabled: true,
    priority: 100,
  });

  await api.post('/functions', {
    name: `seed-normalize-level-${STAMP}`,
    language: 'vrl',
    source: '.level = upcase!(.level)\n.customer_tier = "pro"',
    params_schema: {},
  });
  const extendTableName = `seed_customers_${STAMP}`;
  await api.post('/extend_tables', {
    table_name: extendTableName,
    description: 'Customer tier, region, and ownership enrichment',
    key_field: 'customer_id',
    value_fields: [
      { name: 'tier', field_type: 'string', required: true, description: 'Customer plan tier' },
      { name: 'region', field_type: 'string', required: true, description: 'Customer home region' },
      { name: 'owner', field_type: 'string', required: true, description: 'Owning team' },
    ],
  });
  await api.put(`/extend_tables/${extendTableName}/rows/customer-1001`, {
    value_json: { tier: 'pro', region: 'us-east-1', owner: 'platform' },
  });
  await api.post('/invitations', {
    email: `seed-invite-${STAMP}@example.com`,
    role: 'viewer',
  });
  await api.post('/connectors', {
    name: `seed cloudwatch logs ${STAMP}`,
    kind: 'aws_cloudwatch_logs',
    enabled: false,
    config_json: {
      target_stream: 'app_logs',
      log_group: '/aws/molesignal/seed',
      region: 'us-east-1',
      access_key: 'seed-access-key',
      secret_key: 'seed-secret-key',
    },
  });

  const pipelines = [];
  const enrichPipeline = await api.post('/scheduled_pipelines', {
    name: `seed enrich app logs ${STAMP}`,
    source_stream: 'app_logs',
    target_stream: 'app_logs_enriched',
    function_steps: [{ function_name: `seed-normalize-level-${STAMP}` }],
    cron: 'every:5m',
    lookback_secs: 900,
    enabled: true,
  });
  pipelines.push(enrichPipeline);
  pipelines.push(
    await api.post('/scheduled_pipelines', {
      name: `seed metrics rollup ${STAMP}`,
      source_stream: 'http_requests_total',
      target_stream: 'http_requests_total_5m',
      function_steps: {
        language: 'vrl',
        signal_type: 'metrics',
        sources: ['http_requests_total'],
        sinks: ['http_requests_total_5m'],
        script: '.window = "5m"\n.rollup = "rate"',
        retry_policy: 'exponential',
      },
      cron: 'every:1m',
      lookback_secs: 300,
      enabled: true,
    }),
  );
  pipelines.push(
    await api.post('/scheduled_pipelines', {
      name: `seed trace normalize ${STAMP}`,
      source_stream: 'traces',
      target_stream: 'traces_enriched',
      function_steps: {
        language: 'vrl',
        signal_type: 'traces',
        sources: ['traces'],
        sinks: ['traces_enriched'],
        script: '.attributes.seeded = true\n.attributes.owner = "platform"',
        retry_policy: 'fixed',
      },
      cron: 'every:10m',
      lookback_secs: 1800,
      enabled: true,
    }),
  );
  await api.post(`/scheduled_pipelines/${enrichPipeline.id}/backfill`, {
    start_micros: NOW_US - 900_000_000,
    end_micros: NOW_US,
  });
  if (directDb) {
    seedPipelineRuns(api.orgId, pipelines);
  }

  await api.post('/auth/tokens', {
    name: `seed readonly token ${STAMP}`,
    role: 'viewer',
    expires_in_days: 30,
  });
  await api.post('/users', {
    email: `seed-user-${STAMP}@example.com`,
    display_name: `Seed User ${STAMP}`,
    password: `Seed-${STAMP}-password`,
  });

  await api.post('/dashboards', {
    folder_id: null,
    model: {
      engine: 'molesignal-dashboard',
      id: '',
      uid: `seed-${STAMP}`,
      title: `Seed Telemetry Overview ${STAMP}`,
      tags: ['seed', 'qa', 'telemetry'],
      description: 'Configuration-driven Dashboard Engine seed data.',
      editable: true,
      defaultDashboard: false,
      timezone: 'browser',
      schemaVersion: 2,
      version: 1,
      refresh: '30s',
      time: { from: 'now-1h', to: 'now' },
      timeSettings: {
        defaultFrom: 'now-1h',
        defaultTo: 'now',
        timezone: 'browser',
      },
      refreshSettings: {
        enabled: true,
        mode: 'interval',
        defaultInterval: '30s',
        allowedIntervals: ['off', '5s', '10s', '30s', '1m', '5m'],
      },
      variables: [],
      annotations: [],
      links: [],
      layout: {
        type: 'grid',
        columns: 24,
        rowHeight: 8,
        gap: 8,
      },
      elements: [
        {
          kind: 'panel',
          id: 'seed-request-rate',
          title: 'Request rate',
          gridPos: { x: 0, y: 0, w: 12, h: 24 },
          queryOptions: {},
          queries: [
            {
              refId: 'A',
              enabled: true,
              dataSourceType: 'metrics',
              query: {
                language: 'promql',
                expression: 'rate(http_requests_total[5m])',
              },
            },
          ],
          transformations: [],
          visualization: {
            type: 'time_series',
            schemaVersion: 1,
            options: {},
          },
          fieldConfig: {},
          overrides: [],
          links: [],
        },
        {
          kind: 'panel',
          id: 'seed-error-logs',
          title: 'Checkout errors',
          gridPos: { x: 12, y: 0, w: 12, h: 24 },
          queryOptions: {},
          queries: [
            {
              refId: 'A',
              enabled: true,
              dataSourceType: 'logs',
              query: {
                language: 'sql',
                expression:
                  "SELECT * FROM app_logs WHERE level = 'error' ORDER BY _timestamp DESC LIMIT 50",
                streamName: 'app_logs',
                streamType: 'logs',
              },
            },
          ],
          transformations: [],
          visualization: {
            type: 'logs',
            schemaVersion: 1,
            options: {},
          },
          fieldConfig: {},
          overrides: [],
          links: [],
        },
      ],
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      createdBy: '',
      updatedBy: '',
    },
  });

  return [
    'control-plane: alert escalation/rule, schedule + override, function, ' +
      'extend table row, invitation, connector, 3 scheduled pipelines + backfill, ' +
      'pipeline runs, api token, user, dashboard, notify connector/endpoint/preference, ' +
      'team + organization fallback, template, and policy',
  ];
}

function seedPipelineRuns(orgId, pipelines) {
  const states = ['succeeded', 'succeeded', 'failed', 'succeeded', 'running'];
  const rows = [];
  for (const [pIdx, pipeline] of pipelines.entries()) {
    for (let i = 0; i < states.length; i += 1) {
      const started = NOW_US - (pIdx * 30 + i * 7 + 3) * 60_000_000;
      const running = states[i] === 'running';
      const failed = states[i] === 'failed';
      const finished = running ? 'NULL' : String(started + (18 + i * 3) * 1_000_000);
      const scanned = failed ? 0 : 1200 + pIdx * 430 + i * 125;
      const err = failed ? sqlString('seeded transform error: missing customer_tier') : 'NULL';
      rows.push(`(
        ${sqlString(`seed-run-${STAMP}-${pipeline.id}-${i}`)},
        ${sqlString(pipeline.id)},
        ${sqlString(orgId)},
        ${sqlString(states[i])},
        ${started},
        ${finished},
        ${scanned},
        ${err}
      )`);
    }
  }
  const idsIn = pipelines.map((p) => sqlString(p.id)).join(', ');
  psql(`
    INSERT INTO pipeline_runs
      (id, pipeline_id, org_id, state, started_at_micros, finished_at_micros, scanned_rows, error)
    VALUES ${rows.join(',\n')}
    ON CONFLICT (id) DO NOTHING;

    UPDATE scheduled_pipelines
       SET last_run_at_micros = ${NOW_US - 60_000_000},
           updated_at_micros = ${NOW_US}
     WHERE org_id = ${sqlString(orgId)}
       AND id IN (${idsIn});
  `);
}

// ---------- Verify ----------

async function verify(api, { directDb = true } = {}) {
  const timeRange = { start: NOW_US - 4 * 3600 * 1_000_000, end: NOW_US + 60_000_000 };
  const checks = [];

  const logQuery = await api.post('/query', {
    org_id: api.orgId,
    language: 'sql',
    statement: 'SELECT level, service, message, trace_id FROM app_logs ORDER BY _timestamp DESC LIMIT 5',
    time_range: timeRange,
    stream: { name: 'app_logs', stream_type: 'logs' },
    limit: 5,
  });
  checks.push(['app_logs query rows', logQuery.rows?.length ?? 0]);

  const metricQuery = await api.post('/query', {
    org_id: api.orgId,
    language: 'promql',
    statement: 'rate(http_requests_total[5m])',
    time_range: timeRange,
    limit: 20,
  });
  checks.push(['promql rows', metricQuery.rows?.length ?? 0]);

  const trace = await api.get(`/web/trace/${traceId(0)}`);
  checks.push(['web trace spans', trace.spans?.length ?? 0]);

  const graph = await api.get(`/traces/service_graph?from=${timeRange.start}&to=${timeRange.end}`);
  checks.push(['service graph edges', graph.edges?.length ?? 0]);

  let parquetFileMeta = [];
  if (directDb) {
    const names = STREAM_DEFS.map(([, n]) => sqlString(n)).join(', ');
    parquetFileMeta = psql(
      `
        SELECT stream_type || ':' || stream || '=' || COALESCE(SUM(rows),0)::TEXT
        FROM parquet_file_meta
        WHERE org_id = ${sqlString(api.orgId)}
          AND deleted = FALSE
          AND stream IN (${names})
        GROUP BY stream_type, stream
        ORDER BY stream_type, stream;
      `,
      { capture: true },
    )
      .trim()
      .split('\n')
      .filter(Boolean);
  }

  return { checks, parquetFileMeta };
}

// ---------- Main ----------

async function main() {
  const api = new ApiClient(API_BASE);
  await api.login(LOGIN_EMAIL, LOGIN_PASSWORD);
  const orgs = await api.get('/orgs');
  const defaultOrg =
    orgs.find((o) => o.slug === 'default' || o.name === 'default') ?? orgs[0];
  if (defaultOrg && defaultOrg.id !== api.orgId) {
    await api.selectOrg(defaultOrg.id);
  }

  if (ARGS.has('--topology-only')) {
    const topologyTraces = makeTopologyTraces();
    await api.post('/ingest/traces/topology_traces', topologyTraces);
    console.log(
      JSON.stringify(
        {
          stamp: STAMP,
          api_base: API_BASE,
          org_id: api.orgId,
          created: [`traces: ${topologyTraces.length} canonical spans for service graph`],
          note: 'Service graph edges flush on the next 30-second worker tick.',
        },
        null,
        2,
      ),
    );
    return;
  }

  if (ARGS.has('--rum-only')) {
    const linkedTraces = makeTraces();
    await api.post('/ingest/traces/traces', linkedTraces);
    const created = [
      `traces: ${linkedTraces.length} spans linked from RUM actions`,
      ...(await seedRum(api)),
    ];
    console.log(
      JSON.stringify(
        {
          stamp: STAMP,
          api_base: API_BASE,
          org_id: api.orgId,
          created,
          note:
            'Fresh RUM sessions, actions, errors, Web Vitals, replay payloads, and their linked traces only.',
        },
        null,
        2,
      ),
    );
    return;
  }

  if (ARGS.has('--telemetry-only')) {
    const created = await seedTelemetry(api);
    console.log(
      JSON.stringify(
        {
          stamp: STAMP,
          api_base: API_BASE,
          org_id: api.orgId,
          created,
          note: 'Fresh telemetry only; no control-plane resources were created.',
        },
        null,
        2,
      ),
    );
    return;
  }

  const created = [];
  if (API_ONLY) {
    created.push('streams: schema-on-write through ingest APIs');
  } else {
    created.push(seedStreams(api.orgId));
    resetSeedParquetFileMeta(api.orgId);
  }
  created.push(...(await seedTelemetry(api)));
  created.push(...(await seedProfiles(api)));
  created.push(...(await seedRum(api)));
  if (API_ONLY) {
    created.push('service_graph_edges: derived from ingested traces');
  } else {
    created.push(seedServiceGraph(api.orgId));
  }
  created.push(...(await seedControlPlane(api, { directDb: !API_ONLY })));

  await new Promise((resolve) => setTimeout(resolve, 3500));
  const verification = SKIP_VERIFY ? { skipped: true } : await verify(api, { directDb: !API_ONLY });

  console.log(
    JSON.stringify(
      {
        stamp: STAMP,
        api_base: API_BASE,
        org_id: api.orgId,
        database: PG_DATABASE,
        pg_container: API_ONLY ? null : PG_CONTAINER,
        api_only: API_ONLY,
        created,
        verification,
      },
      null,
      2,
    ),
  );
}

main().catch((err) => {
  const msg = err?.message ?? String(err);
  const cause = err?.cause;
  if (cause) {
    const causeMsg = cause.code ? `${cause.code} ${cause.message ?? ''}`.trim() : String(cause);
    console.error(`${msg} (cause: ${causeMsg})`);
  } else {
    console.error(msg);
  }
  process.exitCode = 1;
});
