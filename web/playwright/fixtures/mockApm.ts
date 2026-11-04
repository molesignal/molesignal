/**
 * The APM pages' backend, as the APM specs and the visual baselines see it:
 * one service (`shop/checkout` in `prod`) with an endpoint (its status codes
 * and where its time goes), a dependency, an error group with where it
 * happened, two versions (the newer did worse), its overview, a firing
 * incident about it, its team, owner and on-call, its runbook, an error
 * grouping rule, and the map of what it calls and what calls it.
 */
import type { Page } from '@playwright/test';

import { FROZEN_NOW_ISO } from './mockBackend';

export const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
export const RANGE = { from: NOW - 3_600_000_000, to: NOW };
export const SERVICE = { namespace: 'shop', name: 'checkout', environment: 'prod' };
export const SERVICE_PATH = '/apm/services/checkout?namespace=shop&environment=prod';
export const TRACE_ID = '0123456789abcdef0123456789abcdef';
export const SPAN_ID = '0123456789abcdef';

const meta = {
  range: RANGE,
  resolution: 'minute',
  projectionStartedAt: RANGE.from,
  lastCompleteBucketAt: NOW - 30_000_000,
  dataQuality: { partial: false, gaps: [], overflowDimensions: [] },
  activationBoundary: false,
};

const traces = { namespace: 'shop', service: 'checkout', environment: 'prod', from: RANGE.from, to: RANGE.to };

function red(requestCount = 1_240, errorCount = 24, p95 = 420_000) {
  return {
    requestCount: requestCount,
    errorCount: errorCount,
    errorRate: requestCount === 0 ? 0 : errorCount / requestCount,
    durationSum: requestCount * 180_000,
    durationAverage: 180_000,
    p50Time: 120_000,
    p95Time: p95,
    p99Time: p95 * 2,
    latencyPartial: false,
    exemplars: [],
  };
}

const sparkline = [0, 1, 2, 3].map((index) => ({
  bucketAt: NOW - (4 - index) * 60_000_000,
  requestCount: 300 + index * 10,
  errorCount: index,
  p95Time: 400_000,
}));

const service = {
  service: SERVICE,
  firstSeenAt: RANGE.from - 86_400_000_000,
  lastSeenAt: NOW - 5_000_000,
  instrumentation: { runtimeLanguage: 'rust', telemetrySdkName: 'opentelemetry', telemetrySdkVersion: '0.29.0', recentInstanceCount: 6 },
  versions: ['1.0.0', '2.0.0'],
  currentVersion: '2.0.0',
  currentVersionSince: NOW - 1_800_000_000,
  red: red(),
  apdexThreshold: 300_000,
  sparkline,
  health: 'warning',
  traces,
  // A rollout: two instances, one on each version.
  profile: {
    instanceCount: 2,
    hostCount: 2,
    versions: [
      { value: '2.0.0', instances: 1 },
      { value: '1.0.0', instances: 1 },
    ],
    languages: [{ value: 'rust', instances: 2 }],
    runtimes: [{ value: 'rustc 1.86.0', instances: 2 }],
    frameworks: [{ value: 'Axum', instances: 2 }],
    regions: [{ value: 'ap-southeast-1', instances: 2 }],
    clusters: [{ value: 'prod-sg', instances: 2 }],
    k8sNamespaces: [{ value: 'shop', instances: 2 }],
    workloads: [{ value: 'checkout', instances: 2 }],
    repositories: [{ value: 'https://github.com/acme/checkout', instances: 2 }],
    commits: [{ value: '9f2c1e7a4b3d', instances: 1 }],
    entryKinds: ['http', 'messaging'],
  },
};

/** The instances of the rollout, one per version, each with what it served, and those past the limit together. */
export const INSTANCES = [
  {
    instanceId: 'checkout-7d9f-abcde',
    version: '2.0.0',
    firstSeenAt: NOW - 1_800_000_000,
    lastSeenAt: NOW - 60_000_000,
    attributes: {
      'k8s.namespace.name': 'shop',
      'k8s.deployment.name': 'checkout',
      'k8s.pod.name': 'checkout-7d9f-abcde',
      'host.name': 'node-2',
      'cloud.region': 'ap-southeast-1',
      'process.runtime.name': 'rustc',
      'process.runtime.version': '1.86.0',
    },
    red: red(620, 12, 410_000),
    sparkline: [],
  },
  {
    instanceId: 'checkout-6c1e-fghij',
    version: '1.0.0',
    firstSeenAt: NOW - 86_400_000_000,
    lastSeenAt: NOW - 120_000_000,
    attributes: {
      'k8s.namespace.name': 'shop',
      'k8s.deployment.name': 'checkout',
      'k8s.pod.name': 'checkout-6c1e-fghij',
      'host.name': 'node-1',
      'cloud.region': 'ap-southeast-1',
      'process.runtime.name': 'rustc',
      'process.runtime.version': '1.86.0',
    },
    red: red(620, 12, 410_000),
    sparkline: [],
  },
  {
    instanceId: '__other__',
    version: null,
    firstSeenAt: NOW - 600_000_000,
    lastSeenAt: NOW - 60_000_000,
    attributes: {},
    red: red(40, 0, 300_000),
    sparkline: [],
  },
];

const transaction = {
  service: SERVICE,
  version: '2.0.0',
  transaction: { name: 'POST /orders', kind: 'http' },
  red: { ...red(880, 18, 510_000), apdex: 0.78, statusCodes: { 'http:201': 840, 'http:400': 22, 'http:503': 18 } },
  apdexThreshold: 300_000,
  totalTime: 158_400_000,
  traces: { ...traces, transaction: 'POST /orders' },
};

const dependency = {
  service: SERVICE,
  version: '2.0.0',
  dependency: { category: 'database', target: 'postgres/orders', operation: 'SELECT' },
  red: { ...red(760, 7, 240_000), timeoutCount: 3, retryCount: 2 },
  totalTime: 98_800_000,
  traces: { ...traces, dependency: 'postgres/orders' },
};

/** The slowest statement of the database, kept as its template. */
const statement = {
  ...dependency,
  dependency: { category: 'database', target: 'postgres/orders', operation: 'SELECT * FROM orders WHERE id = ? AND status IN (?)' },
  red: red(120, 0, 380_000),
  totalTime: 30_000_000,
};

const mapRed = (requests: number, errors: number, p95: number) => ({ requestCount: requests, errorCount: errors, errorRate: requests === 0 ? 0 : errors / requests, p95Time: p95 });

/** The services around checkout, what they call and the topic between checkout and the mailer; one call new, one gone. */
export const MAP = {
  meta,
  nodes: [
    { id: 'service:gateway', kind: 'service', name: 'gateway', namespace: 'edge', environments: ['prod'], red: mapRed(2_400, 2, 120_000), health: 'healthy' },
    { id: 'service:checkout', kind: 'service', name: 'checkout', namespace: 'shop', environments: ['prod'], red: mapRed(1_240, 24, 420_000), health: 'warning' },
    { id: 'service:payments', kind: 'service', name: 'payments', namespace: 'pay', environments: ['prod'], red: mapRed(600, 3, 110_000), health: 'healthy' },
    { id: 'service:mailer', kind: 'service', name: 'mailer', namespace: 'shop', environments: ['prod'], red: mapRed(300, 0, 20_000), health: 'healthy' },
    { id: 'database:postgres/orders', kind: 'external', name: 'postgres/orders', category: 'database', red: mapRed(880, 7, 380_000) },
    { id: 'topic:orders', kind: 'topic', name: 'orders', category: 'messaging', red: mapRed(300, 0, 8_000) },
    { id: 'service:legacy-cart', kind: 'service', name: 'legacy-cart', namespace: 'shop', red: mapRed(0, 0, 0), change: 'gone' },
  ],
  edges: [
    { id: 'service:gateway->service:checkout', source: 'service:gateway', target: 'service:checkout', kind: 'call', red: mapRed(1_240, 24, 430_000) },
    { id: 'service:checkout->service:payments', source: 'service:checkout', target: 'service:payments', kind: 'call', red: mapRed(600, 3, 110_000), change: 'new' },
    { id: 'service:checkout->database:postgres/orders', source: 'service:checkout', target: 'database:postgres/orders', kind: 'call', red: mapRed(880, 7, 380_000) },
    { id: 'service:checkout->topic:orders', source: 'service:checkout', target: 'topic:orders', kind: 'publish', red: mapRed(300, 0, 8_000) },
    { id: 'topic:orders->service:mailer', source: 'topic:orders', target: 'service:mailer', kind: 'consume', red: mapRed(300, 0, 20_000) },
    { id: 'service:gateway->service:legacy-cart', source: 'service:gateway', target: 'service:legacy-cart', kind: 'call', red: mapRed(0, 0, 0), change: 'gone' },
  ],
  namespaces: ['edge', 'pay', 'shop'],
  environments: ['prod', 'staging'],
  truncated: false,
};

/** How long the orders topic's messages waited, from a sample of the trace store. */
export const MESSAGE_WAIT = { sampled: 120, matched: 96, p50Time: 8_000, p95Time: 42_000, maxTime: 180_000 };

const error = {
  error: { fingerprint: 'fp-checkout', errorType: 'PaymentDeclined', applicationFrame: 'checkout::payment', transactionName: 'POST /orders', overflow: false },
  service: SERVICE,
  firstSeenAt: RANGE.from,
  lastSeenAt: NOW - 20_000_000,
  occurrenceCount: 24,
  representativeMessage: 'Payment provider rejected the request',
  red: red(24, 24, 300_000),
  traces: { ...traces, errorFingerprint: 'fp-checkout' },
  issue: null,
};

/** 2.0.0 against 1.0.0: errors up two points and P95 slower, by the rule the server judges by. */
const regressed = {
  previous: '1.0.0',
  status: 'regressed',
  worse: ['error_rate', 'latency'],
  delta: { requestCountAbsolute: 200, requestCountRelative: 0.3846, errorRateAbsolute: 0.0173, errorRateRelative: 2.25, p95AbsoluteTime: 290_000, p95Relative: 1.318 },
};

const versions = [
  { service: SERVICE, version: '2.0.0', firstSeenAt: NOW - 1_800_000_000, lastSeenAt: NOW, observationCount: 720, red: red(720, 18, 510_000), verdict: regressed },
  { service: SERVICE, version: '1.0.0', firstSeenAt: RANGE.from, lastSeenAt: NOW - 1_800_000_000, observationCount: 520, red: red(520, 4, 220_000) },
];

/** What the pipeline reported: 2.0.0 going out, and before it a deployment that failed. */
export const DEPLOYMENTS = [
  {
    id: 'deploy-2',
    namespace: 'shop',
    service: 'checkout',
    environment: 'prod',
    version: '2.0.0',
    status: 'succeeded',
    commit: '9f2c1e7a4b3d5e6f',
    repositoryUrl: 'https://github.com/acme/checkout',
    pullRequestUrl: 'https://github.com/acme/checkout/pull/42',
    author: 'ada',
    ciUrl: 'https://ci.example.com/runs/7',
    deployedAt: NOW - 1_860_000_000,
    createdBy: 'token-1',
    createdAt: NOW - 1_860_000_000,
  },
  {
    id: 'deploy-1',
    namespace: 'shop',
    service: 'checkout',
    environment: 'prod',
    version: '1.9.9',
    status: 'failed',
    commit: '1a2b3c4d5e6f7a8b',
    author: 'ada',
    ciUrl: 'https://ci.example.com/runs/6',
    deployedAt: NOW - 2_400_000_000,
    createdBy: 'token-1',
    createdAt: NOW - 2_400_000_000,
  },
];

/**
 * What insights find: checkout's errors rose since ten minutes ago, right
 * after 2.0.0 went out (also a version that did worse), and where its time
 * could be won back.
 */
export const INSIGHTS = {
  meta,
  now: { requestCount: 1_240, throughput: 0.34, errorRate: 0.0194, p95Time: 420_000, apdex: 0.86 },
  previous: { requestCount: 1_180, throughput: 0.33, errorRate: 0.004, p95Time: 300_000 },
  lastWeek: { requestCount: 1_210, throughput: 0.34, errorRate: 0.003, p95Time: 280_000 },
  insights: [
    {
      kind: 'anomaly',
      severity: 'warning',
      service: SERVICE,
      metric: 'error_rate',
      value: 0.0194,
      previous: 0.004,
      lastWeek: 0.003,
      since: NOW - 600_000_000,
      changes: [
        { kind: 'deployment', source: 'reported', service: 'checkout', version: '2.0.0', status: 'succeeded', at: NOW - 660_000_000, downstream: false, commit: '9f2c1e7a4b3d5e6f' },
        { kind: 'configuration', source: 'reported', service: 'payments', version: '', status: 'succeeded', at: NOW - 900_000_000, downstream: true, description: 'payments.timeout_ms 2000 → 500' },
      ],
    },
    { kind: 'regression', severity: 'warning', service: SERVICE, metric: 'p95', value: 510_000, previous: 220_000, since: NOW - 1_800_000_000, version: '2.0.0', previousVersion: '1.0.0' },
    {
      kind: 'repeated_call',
      severity: 'info',
      service: SERVICE,
      dependency: { category: 'database', target: 'postgres orders', operation: 'SELECT * FROM order_items WHERE order_id = ?' },
      value: 24,
      calls: 29_760,
      p95Time: 4_000,
    },
    { kind: 'slow_dependency', severity: 'info', service: SERVICE, dependency: { category: 'database', target: 'postgres orders' }, value: 0.55, calls: 31_000, p95Time: 8_000 },
    { kind: 'hot_endpoint', severity: 'info', service: SERVICE, transaction: { name: 'POST /orders', kind: 'http' }, value: 0.62, calls: 1_240, p95Time: 420_000 },
  ],
};

/** What checkout reaches along the map: the gateway calls it, it calls payments, which is failing, and postgres. */
export const BLAST_RADIUS = {
  meta,
  service: { id: 'service:checkout', kind: 'service', name: 'checkout', namespace: 'shop', hops: 0, via: 'service:checkout', health: 'warning', red: red(), call: red(0, 0) },
  upstream: [{ id: 'service:gateway', kind: 'service', name: 'gateway', namespace: 'shop', hops: 1, via: 'service:checkout', health: 'healthy', red: red(2_000, 2, 90_000), call: red(1_240, 24, 420_000) }],
  downstream: [
    { id: 'service:payments', kind: 'service', name: 'payments', namespace: 'shop', hops: 1, via: 'service:checkout', health: 'critical', red: red(900, 110, 300_000), call: red(880, 108, 310_000) },
    { id: 'database:postgres orders', kind: 'external', name: 'postgres orders', category: 'database', hops: 1, via: 'service:checkout', health: 'healthy', red: red(31_000, 0, 8_000), call: red(31_000, 0, 8_000) },
  ],
};

/** The organization's APM settings, as an organization that set nothing has them. */
export const APM_SETTINGS = {
  health: { errorRateWarning: 0.02, errorRateCritical: 0.1, p95WarningTime: 500_000, p95CriticalTime: 2_000_000 },
  apdexThreshold: 500_000,
  longTaskTime: 1_000_000,
  namingRules: [],
  environmentMap: [],
  aliases: [],
  metadataRules: { teamFromTier: null, ownerFromTier: null, repositoryFromTier: null },
};

/** Where the error group happened, and every listed group. */
const breakdown = {
  versions: [
    { value: '2.0.0', count: 18 },
    { value: '1.0.0', count: 6 },
  ],
  environments: [{ value: 'prod', count: 24 }],
  languages: [{ value: 'rust', count: 24 }],
};

/** Where the endpoint's latest requests spent their time. */
export const TIME_SPENT = {
  sampled: 120,
  totalTime: 61_200_000,
  parts: [
    { category: 'database', micros: 30_600_000 },
    { category: 'own', micros: 15_300_000 },
    { category: 'http', micros: 9_180_000 },
    { category: 'cache', micros: 6_120_000 },
  ],
};

export const ERROR_RULE = {
  id: 'error-rule-1',
  name: 'Payment timeouts',
  serviceNamespace: 'shop',
  serviceName: 'checkout',
  errorType: '*Timeout*',
  message: '',
  frame: '',
  endpoint: '',
  action: 'merge',
  enabled: true,
  position: 0,
  updatedBy: 'user-1',
  updatedAt: NOW,
};

const trend = [
  { bucketAt: NOW - 120_000_000, red: red(90, 1, 250_000) },
  { bucketAt: NOW - 60_000_000, red: red(110, 3, 420_000) },
];

const distribution = [
  { lowerTime: 100_000, upperTime: 109_000, count: 300 },
  { lowerTime: 380_000, upperTime: 414_000, count: 120 },
  { lowerTime: 800_000, upperTime: 840_000, count: 12 },
];

const definitions = [
  {
    namespace: 'shop',
    name: 'checkout',
    description: 'Takes orders and payments.',
    teamId: 'team-pay',
    ownerId: 'user-1',
    tier: 'p0',
    lifecycle: 'production',
    tags: { domain: 'orders', pci: '' },
    apdexThreshold: 300_000,
    keyTransactions: [] as Array<{ name: string; kind: string }>,
    archived: false,
    updatedBy: 'user-1',
    updatedAt: NOW,
  },
  { namespace: 'shop', name: 'billing', description: '', teamId: null, ownerId: null, tier: 'p1', lifecycle: 'staging', tags: { domain: 'payments' }, updatedBy: 'user-1', updatedAt: NOW },
  { namespace: 'shop', name: 'legacy-cart', description: '', teamId: null, ownerId: null, archived: true, updatedBy: 'user-1', updatedAt: NOW },
];

/** What the service sends besides its traces: logs and profiles, a browser session, no metrics. */
export const COVERAGE = {
  from: NOW - 86_400_000_000,
  to: NOW,
  logs: { state: 'reported', found: ['default'] },
  metrics: { state: 'missing' },
  profiles: { state: 'reported', found: ['cpu'] },
  rum: { state: 'reported', found: ['session-1'] },
};

const page50 = (items: unknown[], sort: string) => ({ meta, items, nextCursor: null, previousCursor: null, hasMore: false, sort });

export const overview = {
  meta,
  red: { ...red(), p90Time: 380_000, apdex: 0.91 },
  trend,
  serviceHealth: { healthy: 0, warning: 1, critical: 0, no_traffic: 1 },
  services: [service],
  topTransactions: [],
  topDependencies: [],
  topErrors: [],
  recentVersions: [],
  serviceCount: 2,
  activeServiceCount: 1,
  healthTrend: trend.map((point) => ({ bucketAt: point.bucketAt, counts: { healthy: 0, warning: 1, critical: 0, no_traffic: 1 } })),
  rankings: {
    slowest: [{ service: SERVICE, health: 'warning', requestCount: 1_240, errorRate: 24 / 1_240, p95Time: 420_000 }],
    most_errors: [{ service: SERVICE, health: 'warning', requestCount: 1_240, errorRate: 24 / 1_240, p95Time: 420_000 }],
    busiest: [{ service: SERVICE, health: 'warning', requestCount: 1_240, errorRate: 24 / 1_240, p95Time: 420_000 }],
  },
  deployments: [versions[0]],
  environments: [{ environment: 'prod', serviceCount: 1, requestCount: 1_240 }],
  availableEnvironments: ['prod', 'staging'],
  instrumentation: {
    sdks: [{ language: 'rust', sdkName: 'opentelemetry', sdkVersion: '0.29.0', serviceCount: 1 }],
    withoutEnvironment: 0,
    withoutVersion: 1,
    withoutInstance: 0,
  },
};

export const SLO = {
  id: 'slo-1',
  name: 'Checkout availability',
  description: '',
  namespace: 'shop',
  service: 'checkout',
  environment: 'prod',
  transaction: null,
  kind: 'availability',
  latencyThreshold: null,
  objective: 0.999,
  windowDays: 30,
  createdBy: 'user-1',
  createdAt: NOW - 86_400_000_000,
  updatedBy: 'user-1',
  updatedAt: NOW - 86_400_000_000,
};

const sloStatus = {
  slo: SLO,
  state: 'at_risk',
  sli: 0.9993,
  goodRequests: 99_930,
  totalRequests: 100_000,
  budgetRemaining: 0.3,
  burnRates: { oneHour: 16.2, sixHours: 4.1, oneDay: 1.8 },
};

const responses: Record<string, unknown> = {
  '/api/v1/apm/slos': [sloStatus],
  '/api/v1/apm/slos/slo-1': {
    status: sloStatus,
    history: [0, 1, 2].map((day) => ({ dayStart: NOW - (3 - day) * 86_400_000_000, sli: 0.9995 - day * 0.0002, goodRequests: 33_310, totalRequests: 33_333 })),
  },
  '/api/v1/apm/overview': overview,
  '/api/v1/apm/services': page50([service], 'request_count'),
  '/api/v1/apm/services/checkout/instances': INSTANCES,
  '/api/v1/apm/services/checkout/coverage': COVERAGE,
  '/api/v1/apm/services/checkout': {
    meta,
    service,
    red: { ...service.red, apdex: 0.86 },
    trend,
    latencyDistribution: distribution,
    transactions: [transaction],
    dependencies: [dependency, statement],
    errors: [error],
    versions,
  },
  '/api/v1/apm/transactions/POST%20%2Forders': { meta, transaction, trend, latencyDistribution: distribution, errors: [error], versions },
  '/api/v1/apm/errors': { ...page50([error], 'last_seen'), breakdown },
  '/api/v1/apm/errors/fp-checkout': {
    meta,
    group: error,
    trend,
    affectedTransactions: [transaction],
    affectedVersions: ['1.0.0', '2.0.0'],
    breakdown,
    representativeStack: ['checkout::payment::authorize', 'checkout::orders::submit'],
    samples: [
      {
        eventTime: NOW - 20_000_000,
        traceId: TRACE_ID,
        spanId: SPAN_ID,
        traceAvailable: false,
        representativeMessage: error.representativeMessage,
        representativeStack: ['checkout::payment::authorize'],
      },
    ],
  },
  '/api/v1/apm/versions/compare': {
    meta,
    baseline: { version: '1.0.0', sampleCount: 520, red: red(520, 4, 220_000) },
    candidate: { version: '2.0.0', sampleCount: 720, red: red(720, 18, 510_000) },
    sufficientData: true,
    status: 'regressed',
    delta: {
      requestCountAbsolute: 200,
      requestCountRelative: 0.3846,
      errorRateAbsolute: 0.0173,
      errorRateRelative: 2.25,
      p95AbsoluteTime: 290_000,
      p95Relative: 1.318,
    },
    regressedTransactions: [transaction],
    regressedErrors: [error],
  },
  '/api/v1/apm/service-definitions': definitions,
  '/api/v1/apm/map': MAP,
  '/api/v1/apm/deployments': DEPLOYMENTS,
  '/api/v1/apm/insights': INSIGHTS,
  '/api/v1/apm/blast-radius': BLAST_RADIUS,
  '/api/v1/apm/settings': APM_SETTINGS,
};

/** A call of the map opened: a call to a service lists the endpoints it reached and their errors. */
function mapEdge(source: string, target: string) {
  const kind = source.startsWith('topic:') ? 'consume' : target.startsWith('topic:') ? 'publish' : 'call';
  const toService = target.startsWith('service:') && kind === 'call';
  return {
    meta,
    kind,
    red: red(880, 18, 510_000),
    trend,
    operations: toService ? [{ name: 'POST /orders', kind: 'http', red: transaction.red }] : [{ name: kind === 'call' ? 'SELECT' : 'publish orders', red: red(300, 0, 8_000) }],
    everyCaller: toService,
    errors: toService ? [error] : [],
  };
}

const topology = {
  nodes: [
    { id: 'gateway', name: 'gateway', errorRate: 0, p95Ms: 30, rps: 4, spanCount: 400 },
    { id: 'checkout', name: 'checkout', errorRate: 0.02, p95Ms: 420, rps: 3, spanCount: 300 },
    { id: 'payments', name: 'payments', errorRate: 0.01, p95Ms: 120, rps: 2, spanCount: 200 },
  ],
  edges: [
    { source: 'gateway', target: 'checkout', rps: 3, errRate: 0.02, p95Ms: 410 },
    { source: 'checkout', target: 'payments', rps: 2, errRate: 0.01, p95Ms: 115 },
  ],
};

export interface Captured {
  issues: unknown[];
  definitions: unknown[];
  /** The stars set or taken off endpoints, as sent. */
  keyTransactions: unknown[];
  /** The PromQL statements the runtime charts asked for. */
  runtimeQueries: string[];
  spanQueries: URL[];
  slos: unknown[];
  /** What was deleted, as `rule:<id>`, `slo:<id>` and `error-rule:<id>`, in order. */
  removed: string[];
  rules: unknown[];
  /** The error grouping rules created or changed, and the orders set, as sent. */
  errorRules: unknown[];
  /** The map's reads, and the calls opened on it as `<source>-><target>`. */
  mapQueries: URL[];
  mapEdges: string[];
}

/** `idle`: the service sent nothing in the range, though it is known; `canary`: two of its versions run at once. */
export async function installApmRoutes(page: Page, options: { runtimeMetrics?: boolean; idle?: boolean; canary?: boolean } = {}): Promise<Captured> {
  const captured: Captured = { issues: [], definitions: [], keyTransactions: [], runtimeQueries: [], spanQueries: [], slos: [], rules: [], removed: [], errorRules: [], mapQueries: [], mapEdges: [] };
  const errorRules: Array<Record<string, unknown>> = [{ ...ERROR_RULE }];
  // Each test starts from the definitions as written above, stars included.
  const kept = JSON.parse(JSON.stringify(definitions)) as typeof definitions;
  const savedRules: Array<Record<string, unknown>> = [];

  await page.route('**/api/v1/apm/**', async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;

    if (path.startsWith('/api/v1/apm/error-rules')) {
      const id = decodeURIComponent(path.split('/').pop() ?? '');
      if (request.method() === 'GET') return route.fulfill({ json: errorRules });
      if (request.method() === 'DELETE') {
        captured.removed.push(`error-rule:${id}`);
        errorRules.splice(0, errorRules.length, ...errorRules.filter((rule) => rule.id !== id));
        return route.fulfill({ status: 204 });
      }
      const body = request.postDataJSON() as Record<string, unknown>;
      captured.errorRules.push(body);
      if (path.endsWith('/order')) {
        const ids = (body.ids as string[]) ?? [];
        errorRules.sort((left, right) => ids.indexOf(left.id as string) - ids.indexOf(right.id as string));
        errorRules.forEach((rule, position) => (rule.position = position));
        return route.fulfill({ json: errorRules });
      }
      const rule = request.method() === 'POST' ? { ...ERROR_RULE, ...body, id: `error-rule-${errorRules.length + 1}`, position: errorRules.length } : { ...errorRules.find((item) => item.id === id), ...body };
      const at = errorRules.findIndex((item) => item.id === rule.id);
      if (at >= 0) errorRules[at] = rule;
      else errorRules.push(rule);
      return route.fulfill({ status: request.method() === 'POST' ? 201 : 200, json: rule });
    }

    if (request.method() === 'PUT' && path.endsWith('/issue')) {
      const body = request.postDataJSON() as { status: string; assigneeId: string | null };
      captured.issues.push(body);
      await route.fulfill({ json: { ...body, regressed: false, resolvedAt: NOW, updatedBy: 'user-1', updatedAt: NOW } });
      return;
    }

    if ((request.method() === 'POST' && path === '/api/v1/apm/slos') || (request.method() === 'PUT' && path.startsWith('/api/v1/apm/slos/'))) {
      const body = request.postDataJSON() as Record<string, unknown>;
      captured.slos.push(body);
      await route.fulfill({ json: { ...SLO, ...body, id: 'slo-2' } });
      return;
    }

    if (request.method() === 'DELETE' && path.startsWith('/api/v1/apm/slos/')) {
      captured.removed.push(`slo:${decodeURIComponent(path.split('/').pop() ?? '')}`);
      await route.fulfill({ status: 204 });
      return;
    }

    if (request.method() === 'POST' && path === '/api/v1/apm/service-definitions/checkout/key-transactions') {
      const body = request.postDataJSON() as { transaction: { name: string; kind: string }; key: boolean };
      captured.keyTransactions.push(body);
      const checkout = kept[0]!;
      const others = (checkout.keyTransactions ?? []).filter((entry) => entry.name !== body.transaction.name || entry.kind !== body.transaction.kind);
      checkout.keyTransactions = body.key ? [...others, body.transaction] : others;
      await route.fulfill({ json: checkout });
      return;
    }

    if (request.method() === 'PUT' && path.startsWith('/api/v1/apm/service-definitions/')) {
      const body = request.postDataJSON() as Record<string, unknown>;
      captured.definitions.push(body);
      await route.fulfill({ json: { keyTransactions: kept[0]!.keyTransactions, ...body, namespace: 'shop', name: 'checkout', updatedBy: 'user-1', updatedAt: NOW } });
      return;
    }

    if (path === '/api/v1/apm/service-definitions') {
      await route.fulfill({ json: kept });
      return;
    }

    if (path === '/api/v1/apm/map/edge') {
      const query = new URL(request.url()).searchParams;
      captured.mapEdges.push(`${query.get('source')}->${query.get('target')}`);
      await route.fulfill({ json: mapEdge(query.get('source') ?? '', query.get('target') ?? '') });
      return;
    }

    if (path === '/api/v1/apm/map') captured.mapQueries.push(new URL(request.url()));

    const detail = path === '/api/v1/apm/services/checkout';
    const body = options.idle && detail ? idleDetail() : options.canary && detail ? canaryDetail() : responses[path];
    await route.fulfill(body ? { json: body } : { status: 404, json: { error: `No APM fixture for ${path}` } });
  });
  await page.route(/\/api\/v1\/teams(?:\?.*)?$/, (route) => route.fulfill({ json: [{ id: 'team-pay', orgId: 'default', name: 'Payments', memberIds: ['user-1'] }] }));
  await page.route(/\/api\/v1\/users(?:\?.*)?$/, (route) =>
    route.fulfill({
      json: [{ id: 'user-1', email: 'ada@example.com', displayName: 'Ada Lovelace', isRoot: false, disabled: false, roles: [], teamNames: ['Payments'], loginMethod: 'password', createdAt: 0 }],
    }),
  );
  await page.route(/\/api\/v1\/schedules(?:\?.*)?$/, (route) =>
    route.fulfill({ json: [{ id: 'sched-1', orgId: 'default', name: 'Payments on call', description: '', teamId: 'team-pay', timezone: 'UTC', enabled: true, rotations: [], overrides: [], createdAt: 0 }] }),
  );
  await page.route(/\/api\/v1\/schedules\/sched-1\/on-call(?:\?.*)?$/, (route) => route.fulfill({ json: { userId: 'user-1' } }));
  await page.route(/\/api\/v1\/agent\/knowledge\/files(?:\?.*)?$/, (route) => {
    // The organization's base holds the service's runbook; the team's holds nothing of it.
    const org = new URL(route.request().url()).searchParams.get('scope') === 'org';
    const files = org ? [{ path: 'services/checkout/runbook.md', sizeBytes: 120, updatedBy: 'user-1', updatedAt: NOW }] : [];
    return route.fulfill({ json: { files, canEdit: true } });
  });
  await page.route(/\/api\/v1\/web\/topology(?:\?.*)?$/, (route) => route.fulfill({ json: topology }));
  // Rules created here are listed back, so pages see the rules they made.
  await page.route(/\/api\/v1\/alerts\/rules$/, async (route) => {
    if (route.request().method() === 'GET') return route.fulfill({ json: savedRules });
    if (route.request().method() !== 'POST') return route.fallback();
    const body = route.request().postDataJSON() as Record<string, unknown>;
    captured.rules.push(body);
    const rule = { ...body, id: `rule-${captured.rules.length}`, orgId: 'default', escalationPolicyId: 'default' };
    savedRules.push(rule);
    return route.fulfill({ json: rule });
  });
  await page.route(/\/api\/v1\/alerts\/rules\/[^/?]+$/, async (route) => {
    if (route.request().method() !== 'DELETE') return route.fallback();
    const id = decodeURIComponent(new URL(route.request().url()).pathname.split('/').pop() ?? '');
    captured.removed.push(`rule:${id}`);
    savedRules.splice(0, savedRules.length, ...savedRules.filter((rule) => rule.id !== id));
    return route.fulfill({ body: 'deleted' });
  });
  await page.route(/\/api\/v1\/alerts\/incidents(?:\?.*)?$/, (route) =>
    route.fulfill({
      json: [
        {
          id: 'inc-1',
          orgId: 'default',
          ruleId: 'rule-1',
          escalationPolicyId: 'esc-1',
          status: 'open',
          severity: 'critical',
          summary: 'Checkout error rate above 2%',
          fingerprint: 'fp',
          currentStep: 0,
          currentLoop: 0,
          currentStepStartedAt: NOW - 600_000_000,
          assignees: [],
          createdAt: NOW - 600_000_000,
          labels: { 'service.name': 'checkout' },
          annotations: {},
          traceIds: [],
          hostIds: [],
          affectedServices: [],
          triggeringQuery: null,
        },
      ],
    }),
  );
  await page.route(/\/api\/v1\/web\/traces\/time-spent(?:\?.*)?$/, (route) => route.fulfill({ json: TIME_SPENT }));
  await page.route(/\/api\/v1\/web\/traces\/messaging-latency(?:\?.*)?$/, (route) => route.fulfill({ json: MESSAGE_WAIT }));
  await page.route(/\/api\/v1\/web\/traces\/spans(?:\?.*)?$/, async (route) => {
    captured.spanQueries.push(new URL(route.request().url()));
    await route.fulfill({
      json: {
        items: [{ traceId: TRACE_ID, spanId: SPAN_ID, service: 'checkout', operation: 'POST /orders', startNs: (NOW - 60_000_000) * 1_000, durationNs: 812_000_000, status: 'OK', kind: 2 }],
        nextCursor: null,
        previousCursor: null,
        hasMore: false,
      },
    });
  });
  await page.route(/\/api\/v1\/metrics\/catalog(?:\?.*)?$/, async (route) => {
    const family = new URL(route.request().url()).searchParams.get('q');
    const runtime: Record<string, string> = { process_: 'process_cpu_utilization_ratio', tokio_: 'tokio_num_alive_tasks' };
    const items =
      options.runtimeMetrics && family && runtime[family]
        ? [{ name: runtime[family], metricType: 'gauge', labels: ['service.name', 'service.instance.id'], fieldCount: 3 }]
        : [];
    await route.fulfill({ json: { items, nextCursor: null, previousCursor: null, hasMore: false } });
  });
  await page.route('**/api/v1/query', async (route) => {
    const body = route.request().postDataJSON() as { statement?: string } | null;
    if (body?.statement) captured.runtimeQueries.push(body.statement);
    await route.fulfill({
      json: {
        columns: ['_timestamp', 'value', 'service.instance.id'],
        rows: [
          [NOW - 120_000_000, 0.42, 'checkout-01'],
          [NOW - 60_000_000, 0.47, 'checkout-01'],
        ],
        scannedRows: 2,
        tookMs: 1,
      },
    });
  });

  if (options.runtimeMetrics) {
    const profiles = ['cpu', 'alloc_space'].map((type, index) => ({
      id: `prof-${index}`,
      service: 'checkout',
      profileType: type,
      timestamp: NOW - (index + 1) * 60_000_000,
      totalValue: 1_000_000_000,
      sampleCount: 100,
      durationNanos: 10_000_000_000,
      unsymbolized: false,
    }));
    await page.route(/\/api\/v1\/profiles(?:\?.*)?$/, (route) => route.fulfill({ json: { profiles } }));
    await page.route(/\/api\/v1\/profiles\/flamegraph(?:\?.*)?$/, (route) => route.fulfill({ json: { flamebearer: FLAMEBEARER, truncated: false, profileCount: 1 } }));
  }

  return captured;
}

const FLAMEBEARER = {
  names: ['total', 'tokio::runtime::poll', 'checkout::orders::submit'],
  // offset, total, self, name
  levels: [
    [0, 100, 0, 0],
    [0, 100, 20, 1],
    [0, 80, 80, 2],
  ],
  numTicks: 100,
  maxSelf: 80,
  units: 'nanoseconds',
};

/** The service's detail as the mock serves it, for a test that serves a variant of it. */
export function serviceDetailFixture() {
  return JSON.parse(JSON.stringify(responses['/api/v1/apm/services/checkout'])) as Record<string, unknown>;
}

/** The service's detail when nothing came from it in the range. */
/** 1.0.0 still running beside 2.0.0, as a canary rollout has them. */
function canaryDetail() {
  const detail = responses['/api/v1/apm/services/checkout'] as { versions: typeof versions };
  return { ...detail, versions: versions.map((version) => (version.version === '1.0.0' ? { ...version, lastSeenAt: NOW - 60_000_000 } : version)) };
}

function idleDetail() {
  const detail = responses['/api/v1/apm/services/checkout'] as { service: typeof service };
  return { ...detail, service: { ...detail.service, red: red(0, 0), health: 'no_traffic', healthReasons: [] } };
}
