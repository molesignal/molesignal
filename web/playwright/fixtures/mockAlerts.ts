/**
 * Mock data for the alert centre: the rules that are evaluated, the incidents
 * they opened, and who is told. It describes the same made-up company as
 * `mockReliability.ts`, at the same moment: an edge proxy change is failing
 * requests to the public API, which is what most of the open incidents are
 * about.
 *
 * Every time is relative to the mock session's frozen clock.
 */
import type { Express } from 'express';

import type { AlertInsights } from '../../src/api/incidents';
import type { SemanticGroup } from '../../src/api/semanticGroups';
import type {
  AlertRule,
  EscalationPolicy,
  Incident,
  IncidentRca,
  MuteRule,
  Schedule,
  Severity,
} from '../../src/types/alerting';

import { scheduleCoverage, withCoverage } from './scheduleCoverage';

const ORG = 'acme-prod';
const MINUTE = 60_000_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

interface RuleSeed {
  id: string;
  name: string;
  description: string;
  service: string;
  team: string;
  severity: Severity;
  kind?: AlertRule['kind'];
  language: 'sql' | 'promql';
  statement: string;
  stream: { name: string; streamType: 'logs' | 'metrics' | 'traces' };
  threshold: number;
  state: 'healthy' | 'pending' | 'firing';
  enabled?: boolean;
}

const RULE_SEEDS: RuleSeed[] = [
  { id: 'rule-api-5xx', name: 'Public API 5xx rate', description: 'Share of requests answered with a 5xx.', service: 'api-gateway', team: 'platform', severity: 'critical', language: 'promql', statement: 'sum(rate(http_requests_total{service="api-gateway",code=~"5.."}[5m])) / sum(rate(http_requests_total{service="api-gateway"}[5m])) * 100', stream: { name: 'http_requests_total', streamType: 'metrics' }, threshold: 2, state: 'firing' },
  { id: 'rule-checkout-latency', name: 'Checkout p95 latency', description: 'p95 of the checkout transaction.', service: 'checkout', team: 'payments', severity: 'error', language: 'promql', statement: 'histogram_quantile(0.95, sum by (le) (rate(http_server_duration_bucket{service="checkout"}[5m])))', stream: { name: 'http_server_duration_bucket', streamType: 'metrics' }, threshold: 800, state: 'firing' },
  { id: 'rule-dashboard-errors', name: 'Dashboard error logs', description: 'Errors logged by the web dashboard.', service: 'web-dashboard', team: 'frontend', severity: 'warning', language: 'sql', statement: "SELECT COUNT(*) FROM \"app_logs\" WHERE \"service.name\" = 'web-dashboard' AND level = 'ERROR'", stream: { name: 'app_logs', streamType: 'logs' }, threshold: 50, state: 'firing' },
  { id: 'rule-ingest-lag', name: 'Ingestion lag', description: 'Seconds between receipt and searchability.', service: 'ingester', team: 'platform', severity: 'warning', language: 'promql', statement: 'max(ingest_lag_seconds)', stream: { name: 'ingest_lag_seconds', streamType: 'metrics' }, threshold: 120, state: 'pending' },
  { id: 'rule-payments-declines', name: 'Payment declines', description: 'Unusual share of declined authorizations.', service: 'payments', team: 'payments', severity: 'error', kind: 'anomaly', language: 'promql', statement: 'sum(rate(payment_authorizations_total{result="declined"}[10m]))', stream: { name: 'payment_authorizations_total', streamType: 'metrics' }, threshold: 0, state: 'healthy' },
  { id: 'rule-db-connections', name: 'Postgres connections', description: 'Connections in use on the primary.', service: 'postgres', team: 'platform', severity: 'warning', language: 'promql', statement: 'max(pg_stat_activity_count{role="primary"})', stream: { name: 'pg_stat_activity_count', streamType: 'metrics' }, threshold: 380, state: 'healthy' },
  { id: 'rule-slow-traces', name: 'Slow order traces', description: 'Order traces slower than five seconds.', service: 'orders', team: 'orders', severity: 'warning', language: 'sql', statement: 'SELECT COUNT(DISTINCT trace_id) FROM "traces" WHERE "service.name" = \'orders\' AND duration_ns > 5000000000', stream: { name: 'traces', streamType: 'traces' }, threshold: 20, state: 'healthy' },
  { id: 'rule-cert-expiry', name: 'Certificate expiry', description: 'Days left on the edge certificate.', service: 'edge', team: 'platform', severity: 'info', language: 'promql', statement: 'min(tls_certificate_days_remaining{host="app.acme.example"})', stream: { name: 'tls_certificate_days_remaining', streamType: 'metrics' }, threshold: 21, state: 'healthy', enabled: false },
];

function rules(now: number): AlertRule[] {
  return RULE_SEEDS.map((seed, index) => ({
    id: seed.id,
    orgId: ORG,
    name: seed.name,
    description: seed.description,
    enabled: seed.enabled ?? true,
    kind: seed.kind ?? 'scheduled',
    query: { language: seed.language, statement: seed.statement, periodSecs: 60, stream: seed.stream },
    trigger: { operator: seed.id === 'rule-cert-expiry' ? 'lt' : 'gt', threshold: seed.threshold, forPeriods: 3, silenceSecs: 600 },
    thresholds:
      seed.severity === 'critical'
        ? [
            { severity: 'warning', operator: 'gt', threshold: seed.threshold / 2, forPeriods: 3 },
            { severity: 'critical', operator: 'gt', threshold: seed.threshold, forPeriods: 3 },
          ]
        : [],
    severity: seed.severity,
    anomalyParams: seed.kind === 'anomaly' ? { algorithm: 'mad', lookbackDays: 14, k: 3, weeklySeasonality: true } : null,
    escalationPolicyId: seed.team === 'payments' ? 'esc-payments' : 'esc-platform',
    labels: { service: seed.service, team: seed.team },
    annotations: { runbook: `https://runbooks.acme.example/${seed.id}` },
    lastEvalAt: seed.enabled === false ? null : now - (index + 1) * 7_000_000,
    lastState: seed.state === 'pending' ? { kind: 'pending', consecutive: 2 } : { kind: seed.state },
    createdAt: now - (120 - index * 9) * DAY,
    updatedAt: now - (index + 2) * DAY,
  }));
}

interface IncidentSeed {
  id: string;
  ruleId: string;
  summary: string;
  severity: Severity;
  status: Incident['status'];
  /** Micros before now. */
  opened: number;
  acknowledged?: number;
  resolved?: number;
}

const INCIDENT_SEEDS: IncidentSeed[] = [
  { id: 'inc-2291', ruleId: 'rule-api-5xx', summary: 'Public API answers 16% of requests with 502', severity: 'critical', status: 'acknowledged', opened: 38 * MINUTE, acknowledged: 33 * MINUTE },
  { id: 'inc-2292', ruleId: 'rule-checkout-latency', summary: 'Checkout p95 at 2.4 s, three times its usual', severity: 'error', status: 'open', opened: 29 * MINUTE },
  { id: 'inc-2293', ruleId: 'rule-dashboard-errors', summary: 'Dashboard is logging 340 errors a minute', severity: 'warning', status: 'open', opened: 24 * MINUTE },
  { id: 'inc-2288', ruleId: 'rule-ingest-lag', summary: 'Ingestion ran four minutes behind', severity: 'warning', status: 'resolved', opened: 9 * HOUR, acknowledged: 9 * HOUR - 6 * MINUTE, resolved: 8 * HOUR + 10 * MINUTE },
  { id: 'inc-2284', ruleId: 'rule-payments-declines', summary: 'Declines doubled for cards issued in Brazil', severity: 'error', status: 'resolved', opened: 19 * HOUR, acknowledged: 19 * HOUR - 4 * MINUTE, resolved: 18 * HOUR - 20 * MINUTE },
  { id: 'inc-2279', ruleId: 'rule-db-connections', summary: 'Primary reached 392 of 400 connections', severity: 'warning', status: 'resolved', opened: 1 * DAY + 14 * HOUR, acknowledged: 1 * DAY + 14 * HOUR - 11 * MINUTE, resolved: 1 * DAY + 13 * HOUR },
  { id: 'inc-2271', ruleId: 'rule-api-5xx', summary: 'Public API failed for one availability zone', severity: 'critical', status: 'resolved', opened: 2 * DAY + 7 * HOUR, acknowledged: 2 * DAY + 7 * HOUR - 2 * MINUTE, resolved: 2 * DAY + 6 * HOUR + 22 * MINUTE },
  { id: 'inc-2266', ruleId: 'rule-slow-traces', summary: '31 order traces took longer than five seconds', severity: 'warning', status: 'resolved', opened: 3 * DAY + 2 * HOUR, resolved: 3 * DAY + 1 * HOUR + 40 * MINUTE },
  { id: 'inc-2259', ruleId: 'rule-checkout-latency', summary: 'Checkout p95 above 800 ms during the sale', severity: 'error', status: 'resolved', opened: 4 * DAY + 11 * HOUR, acknowledged: 4 * DAY + 11 * HOUR - 3 * MINUTE, resolved: 4 * DAY + 9 * HOUR },
  { id: 'inc-2251', ruleId: 'rule-dashboard-errors', summary: 'Dashboard errors after the 14:00 release', severity: 'warning', status: 'resolved', opened: 5 * DAY + 4 * HOUR, acknowledged: 5 * DAY + 4 * HOUR - 9 * MINUTE, resolved: 5 * DAY + 3 * HOUR + 12 * MINUTE },
  { id: 'inc-2244', ruleId: 'rule-ingest-lag', summary: 'Ingestion lag while a broker was replaced', severity: 'warning', status: 'resolved', opened: 6 * DAY + 3 * HOUR, resolved: 6 * DAY + 1 * HOUR },
  { id: 'inc-2240', ruleId: 'rule-api-5xx', summary: 'Brief 5xx spike on deploy', severity: 'error', status: 'closed', opened: 6 * DAY + 16 * HOUR, resolved: 6 * DAY + 15 * HOUR + 52 * MINUTE },
];

function incident(seed: IncidentSeed, now: number, full: boolean): Incident {
  const rule = RULE_SEEDS.find((candidate) => candidate.id === seed.ruleId)!;
  const createdAt = now - seed.opened;
  const samples = Array.from({ length: 12 }, (_, index) => ({
    ts: createdAt - (11 - index) * MINUTE,
    value: Number((rule.threshold * (0.6 + index * 0.11)).toFixed(2)),
    labels: { service: rule.service },
  }));

  return {
    id: seed.id,
    orgId: ORG,
    ruleId: seed.ruleId,
    escalationPolicyId: rule.team === 'payments' ? 'esc-payments' : 'esc-platform',
    status: seed.status,
    severity: seed.severity,
    summary: seed.summary,
    fingerprint: `${seed.ruleId}:${rule.service}`,
    currentStep: seed.status === 'open' ? 1 : 0,
    currentLoop: 0,
    currentStepStartedAt: createdAt + 5 * MINUTE,
    assignees: seed.acknowledged === undefined ? [] : ['u1'],
    createdAt: createdAt,
    ...(seed.acknowledged === undefined ? {} : { acknowledgedAt: now - seed.acknowledged, acknowledgedBy: 'u1' }),
    ...(seed.resolved === undefined ? {} : { resolvedAt: now - seed.resolved, resolvedBy: 'u1' }),
    labels: { service: rule.service, team: rule.team, env: 'production', rule: rule.name },
    annotations: { runbook: `https://runbooks.acme.example/${rule.id}` },
    traceIds: full ? ['7f3c9a1e5b2d4c6f8a0b1c2d3e4f5a6b', '1b2c3d4e5f60718293a4b5c6d7e8f901'] : ['7f3c9a1e5b2d4c6f8a0b1c2d3e4f5a6b'],
    hostIds: full ? ['edge-proxy-3', 'edge-proxy-7'] : ['edge-proxy-3'],
    affectedServices: full ? [rule.service, 'web-dashboard'] : [rule.service],
    triggeringQuery: full ? { language: rule.language, statement: rule.statement, sampleValues: samples } : null,
  };
}

function insights(now: number, windowSecs: number): AlertInsights {
  const inWindow = INCIDENT_SEEDS.filter((seed) => seed.opened <= windowSecs * 1_000_000);
  const closed = inWindow.filter((seed) => seed.resolved !== undefined);
  const count = (key: (seed: IncidentSeed) => string) => {
    const counts = new Map<string, number>();
    for (const seed of inWindow) counts.set(key(seed), (counts.get(key(seed)) ?? 0) + 1);

    return [...counts.entries()].map(([name, total]) => ({ key: name, count: total })).sort((left, right) => right.count - left.count);
  };
  const byHour = Array.from({ length: 24 }, () => 0);
  for (const seed of inWindow) byHour[new Date((now - seed.opened) / 1_000).getUTCHours()]! += 1;

  return {
    windowSecs: windowSecs,
    total: inWindow.length,
    active: inWindow.length - closed.length,
    closed: closed.length,
    mttrSecs: closed.length === 0 ? 0 : Math.round(closed.reduce((total, seed) => total + (seed.opened - seed.resolved!), 0) / closed.length / 1_000_000),
    noiseRate: 0.17,
    byHour: byHour,
    bySeverity: Object.fromEntries(count((seed) => seed.severity).map(({ key, count: total }) => [key, total])),
    topServices: count((seed) => RULE_SEEDS.find((rule) => rule.id === seed.ruleId)!.service),
    topRules: count((seed) => seed.ruleId),
    response: {
      current: response(inWindow.length, 0.25),
      previous: response(Math.max(1, inWindow.length - 2), 0.4),
      breakdown: {
        teams: [{ key: 'team-platform', incidents: 3, mttaSecs: 240, mttrSecs: 1_800, escalationRate: 0.33 }],
        services: [{ key: 'api-gateway', incidents: 2, mttaSecs: 180, mttrSecs: 2_400, escalationRate: 0.5 }],
        people: [{ key: 'u2', incidents: 2, mttaSecs: 150, mttrSecs: 1_500, escalationRate: 0 }],
        priorities: [{ key: 'P1', incidents: 1, mttaSecs: 90, mttrSecs: 1_200, escalationRate: 0 }],
      },
    },
  };
}

/** How people responded to `incidents` incidents, `escalated` of them escalating. */
function response(incidents: number, escalated: number): AlertInsights['response']['current'] {
  return {
    incidents,
    responded: Math.max(0, incidents - 1),
    mtta: { meanSecs: 240, p50Secs: 180, p90Secs: 600 },
    mttr: { meanSecs: 2_100, p50Secs: 1_500, p90Secs: 5_400 },
    escalationRate: escalated,
    steps: [
      { step: 0, reached: incidents, acknowledged: Math.max(0, incidents - 2) },
      { step: 1, reached: 2, acknowledged: 1 },
    ],
    ackVia: { web: 2, im: 1, sms: 1 },
  };
}

const ESCALATIONS: EscalationPolicy[] = [
  {
    id: 'esc-platform',
    orgId: ORG,
    name: 'Platform on-call',
    steps: [
      { targets: [{ kind: 'schedule', scheduleId: 'sch-platform' }], ackTimeoutSecs: 300, minSeverity: null },
      { targets: [{ kind: 'user', userId: 'u1' }], ackTimeoutSecs: 600, minSeverity: 'error' },
      { targets: [{ kind: 'team', teamId: 'team-platform' }], ackTimeoutSecs: 900, minSeverity: 'critical' },
    ],
    repeat: true,
    maxLoops: 3,
  },
  {
    id: 'esc-payments',
    orgId: ORG,
    name: 'Payments',
    steps: [
      { targets: [{ kind: 'schedule', scheduleId: 'sch-payments' }], ackTimeoutSecs: 300, minSeverity: null },
      { targets: [{ kind: 'user', userId: 'u1' }], ackTimeoutSecs: 900, minSeverity: 'error' },
    ],
    repeat: false,
    maxLoops: 1,
  },
];

function schedules(now: number): Schedule[] {
  const schedule = (id: string, name: string, description: string, members: string[], overrides: Schedule['overrides'] = []): Schedule => ({
    id,
    orgId: ORG,
    name,
    description,
    timezone: 'UTC',
    enabled: true,
    rotations: [
      { id: `${id}-primary`, name: 'Primary', members, kind: 'weekly', activeWindows: [], startAt: now - 60 * DAY },
      { id: `${id}-daytime`, name: 'Business hours backup', members: [...members].reverse(), kind: 'daily', activeWindows: [{ weekdayMask: 0b0111110, start: '09:00', end: '18:00' }], startAt: now - 60 * DAY },
    ],
    overrides,
    createdBy: 'u1',
    updatedBy: 'u1',
    createdAt: now - 60 * DAY,
    updatedAt: now - 3 * DAY,
  });

  return [
    schedule('sch-platform', 'Platform on-call', 'Edge, ingestion and storage.', ['u1', 'u2', 'u3'], [
      { id: 'ovr-1', userId: 'u2', startAt: now - 2 * HOUR, endAt: now + 6 * HOUR, reason: 'Covering while Dana is travelling' },
    ]),
    schedule('sch-payments', 'Payments on-call', 'Checkout and the payment providers.', ['u3', 'u1']),
  ];
}

function findings(now: number) {
  const finding = (id: string, fields: Record<string, unknown>) => ({
    id,
    orgId: ORG,
    severity: 'warning',
    environment: 'production',
    metric: null,
    value: null,
    baseline: null,
    startedAt: now - 40 * MINUTE,
    lastSeenAt: now - 2 * MINUTE,
    endedAt: null,
    missedRounds: 0,
    detail: {},
    createdAt: now - 40 * MINUTE,
    updatedAt: now - 2 * MINUTE,
    ...fields,
  });

  return [
    finding('fd-checkout', {
      kind: 'apm_anomaly',
      key: 'apm_anomaly|default/checkout|production|error_rate',
      severity: 'critical',
      subject: 'default/checkout',
      metric: 'error_rate',
      title: 'Error rate of checkout moved',
      value: 0.12,
      baseline: 0.004,
      detail: {
        previous: 0.004,
        lastWeek: 0.005,
        changes: [{ kind: 'deployment', source: 'deployment', service: 'checkout', version: '2.14.0', status: 'succeeded', at: now - 45 * MINUTE }],
      },
    }),
    finding('fd-web', {
      kind: 'rum_new_issue',
      key: 'rum_new_issue|web-shop|a1b2',
      subject: 'web-shop',
      environment: null,
      title: 'Web shop: TypeError: cart is undefined',
      detail: { issueId: 'a1b2', application: 'Web shop', errorType: 'TypeError', location: 'cart.js:42 · render' },
    }),
    finding('fd-old', {
      kind: 'apm_regression',
      key: 'apm_regression|default/search|production|1.8.0',
      subject: 'default/search',
      title: 'search 1.8.0 does worse than the version before',
      detail: { version: '1.8.0', previousVersion: '1.7.3' },
      endedAt: now - 3 * HOUR,
    }),
  ];
}

function mutes(now: number): MuteRule[] {
  const mute = (id: string, name: string, comment: string, matchers: MuteRule['matchers'], window: MuteRule['window'], enabled = true): MuteRule => ({
    id,
    orgId: ORG,
    name,
    enabled,
    matchers,
    window,
    comment,
    createdBy: 'u1',
    createdAt: now - 20 * DAY,
    updatedAt: now - 2 * DAY,
  });

  return [
    mute('mute-eu-upgrade', 'EU database upgrade', 'Planned upgrade of the EU primary.', [{ label: 'region', op: 'eq', value: 'eu-central-1' }], { type: 'fixed', start: now - 20 * MINUTE, end: now + 100 * MINUTE }),
    mute('mute-staging-nights', 'Staging at night', 'Staging is scaled down outside working hours.', [{ label: 'env', op: 'eq', value: 'staging' }], { type: 'weekly', timezone: 'UTC', ranges: [{ weekdayMask: 0b1111111, start: '20:00', end: '06:00' }] }),
    mute('mute-load-test', 'Load test, March', 'Kept for reference.', [{ label: 'service', op: 're', value: 'checkout|payments' }], { type: 'fixed', start: now - 70 * DAY, end: now - 69 * DAY }, false),
  ].map((rule) => withStanding(rule, now));
}

/** What the server adds to a silence: whether its window covers now, and when that next changes. */
function withStanding(rule: MuteRule, now: number): MuteRule {
  const { window } = rule;
  if (window.type === 'fixed') {
    const next = [window.start, window.end].find((at) => at > now) ?? null;
    return { ...rule, active: window.start <= now && now < window.end, nextChangeAt: next };
  }
  const segments = scheduleCoverage(
    {
      id: rule.id,
      orgId: ORG,
      name: rule.name,
      description: '',
      timezone: 'UTC',
      enabled: true,
      rotations: window.type === 'weekly'
        ? [{ id: 'window', name: 'window', members: ['window'], kind: 'daily', activeWindows: window.ranges, startAt: 0 }]
        : [],
      overrides: [],
      createdAt: 0,
      updatedAt: 0,
    },
    now,
    now + 32 * DAY,
  ).segments;
  return {
    ...rule,
    active: segments[0]?.source === 'rotation',
    nextChangeAt: segments[1]?.startAt ?? null,
  };
}

function semanticGroups(now: number): SemanticGroup[] {
  return [
    { id: 'grp-edge', orgId: ORG, name: 'Edge and API', enabled: true, matchers: [{ label: 'team', op: 'eq', value: 'platform' }], groupBy: ['service', 'env'], createdAt: now - 40 * DAY, updatedAt: now - 6 * DAY },
    { id: 'grp-payments', orgId: ORG, name: 'Money path', enabled: true, matchers: [{ label: 'service', op: 're', value: 'checkout|payments' }], groupBy: ['service'], createdAt: now - 30 * DAY, updatedAt: now - 30 * DAY },
  ];
}

/** Whether the mock has an incident by this id. */
export function mockIncidentExists(id: string): boolean {
  return INCIDENT_SEEDS.some((seed) => seed.id === id);
}

/** An analysis the way the backend writes one for an incident. */
function rootCauseAnalysis(incidentId: string, now: number): IncidentRca {
  return {
    incidentId: incidentId,
    orgId: ORG,
    summary:
      '## Likely cause\n\nThe edge proxy rollout at 09:04 dropped the keep-alive pool for the public API, so 16% of requests get a 502.\n\n- Roll back the edge proxy change.',
    provider: 'openai',
    model: 'gpt-4o',
    promptBuiltinKey: 'incident_rca',
    promptHash: 'rca-hash-1',
    promptTokens: 6_400,
    completionTokens: 820,
    finishReason: 'stop',
    createdAt: now,
    updatedAt: now,
  };
}

/**
 * Registers the alert centre's endpoints on the mock backend. An incident has
 * no root cause analysis until one is asked for; asking is refused the way the
 * backend refuses it while `budgetRefusal` says the Mole Agent budget is used
 * up.
 */
export function registerAlertRoutes(
  app: Express,
  nowIso: string,
  budgetRefusal: () => Record<string, unknown> | null = () => null,
): void {
  const now = Date.parse(nowIso) * 1_000;
  const seedOf = (id: string) => INCIDENT_SEEDS.find((seed) => seed.id === id) ?? INCIDENT_SEEDS[0]!;
  const analyses = new Map<string, IncidentRca>();

  app.get('/api/v1/alerts/rules', (_req, res) => res.json(rules(now)));
  app.get('/api/v1/alerts/rules/:id', (req, res) =>
    res.json(rules(now).find((rule) => rule.id === req.params.id) ?? rules(now)[0]),
  );
  app.get('/api/v1/alerts/incidents', (req, res) => {
    const active = req.query.scope === 'active';
    const windowSecs = Number(req.query.windowSecs ?? 0);
    res.json(
      INCIDENT_SEEDS.filter((seed) => !active || seed.resolved === undefined)
        .filter((seed) => !windowSecs || seed.opened <= windowSecs * 1_000_000)
        .map((seed) => incident(seed, now, false)),
    );
  });
  app.get('/api/v1/alerts/incidents/:id', (req, res) => res.json(incident(seedOf(req.params.id), now, true)));
  app.get('/api/v1/alerts/incidents/:id/rca', (req, res) => {
    const analysis = analyses.get(req.params.id);
    if (!analysis) return res.status(404).json({ error: 'not_found', message: 'no analysis yet' });
    return res.json(analysis);
  });
  app.post('/api/v1/alerts/incidents/:id/rca', (req, res) => {
    const refusal = budgetRefusal();
    if (refusal) return res.status(429).json(refusal);
    const analysis = rootCauseAnalysis(req.params.id, now);
    analyses.set(req.params.id, analysis);
    return res.json(analysis);
  });
  // No incident here is like another and nothing was reported near them; a spec that needs them routes its own.
  app.get('/api/v1/alerts/incidents/:id/similar', (_req, res) => res.json({ incidents: [] }));
  app.get('/api/v1/alerts/incidents/:id/changes', (_req, res) => res.json({ from: 0, to: 0, changes: [] }));
  // Nothing has been done to an incident here; a spec that needs a timeline routes its own.
  app.get('/api/v1/alerts/incidents/:id/timeline', (_req, res) =>
    res.json({ entries: [], deliveries: [], merged: [] }),
  );
  app.get('/api/v1/alerts/insights', (req, res) => res.json(insights(now, Number(req.query.windowSecs ?? 7 * 86_400))));
  // Two people on call in the window; one was paged twice at night.
  app.get('/api/v1/alerts/insights/oncall-load', (req, res) =>
    res.json({
      windowSecs: Number(req.query.windowSecs ?? 30 * 86_400),
      nightStartHour: 22,
      nightEndHour: 8,
      people: [
        { userId: 'u2', oncallSecs: 7 * 86_400, pages: 5, nightPages: 2, weekendPages: 1, interruptedNights: 2, timezone: 'Asia/Shanghai', responded: 3, mttaSecs: 150 },
        { userId: 'u3', oncallSecs: 7 * 86_400, pages: 1, nightPages: 0, weekendPages: 0, interruptedNights: 0, timezone: 'Asia/Shanghai', responded: 1, mttaSecs: 420 },
      ],
    }),
  );
  app.get('/api/v1/alerts/escalations', (_req, res) => res.json(ESCALATIONS));
  app.get('/api/v1/alerts/escalations/:id', (req, res) =>
    res.json(ESCALATIONS.find((policy) => policy.id === req.params.id) ?? ESCALATIONS[0]),
  );
  app.get('/api/v1/alerts/mutes', (_req, res) => res.json(mutes(now)));
  app.get('/api/v1/alerts/inhibit-rules', (_req, res) => res.json([]));
  // Discovery found a move of the checkout errors and a new front-end error; one finding ended.
  app.get('/api/v1/alerts/discovery/findings', (req, res) => {
    const status = String(req.query.status ?? 'open');
    const all = findings(now);
    res.json(status === 'all' ? all : all.filter((finding) => (status === 'ended') === Boolean(finding.endedAt)));
  });
  // Everyone here has a way to get alerts; a spec about someone who has not routes its own.
  app.get('/api/v1/notify/reachability', (req, res) =>
    res.json({
      users: String(req.query.userIds ?? '')
        .split(',')
        .filter(Boolean)
        .map((userId) => ({ userId: userId, status: 'personal' })),
    }),
  );
  // No outside system sends alerts in this company; a spec that needs one routes its own.
  app.get('/api/v1/alerts/sources', (_req, res) => res.json([]));
  app.get('/api/v1/alerts/sources/clusters', (_req, res) => res.json({ local: '', clusters: [] }));
  // Nothing is routed by a rule here and no team has an on-call policy; a spec that needs them routes its own.
  app.get('/api/v1/alerts/routing/rules', (_req, res) => res.json([]));
  app.get('/api/v1/alerts/routing/teams', (_req, res) => res.json([]));
  app.get('/api/v1/alerts/semantic_groups', (_req, res) => res.json(semanticGroups(now)));
  // No incident here is a major one or has a postmortem; a spec that needs them routes its own.
  app.get('/api/v1/alerts/incidents/:id/response', (_req, res) => res.json({ response: null, tasks: [] }));
  app.get('/api/v1/alerts/incidents/:id/postmortem', (_req, res) => res.json({ postmortem: null, actions: [] }));
  app.get('/api/v1/alerts/postmortems', (_req, res) => res.json({ postmortems: [] }));
  app.get('/api/v1/alerts/response/settings', (_req, res) =>
    res.json({
      warRoomConnectorId: null,
      meeting: 'none',
      meetingUrl: '',
      meetingConnectorId: null,
      tencentMeeting: { appId: '', sdkId: '', secretId: '', userid: '' },
      zoom: { accountId: '', clientId: '', host: '' },
      googleMeet: { organizer: '' },
      checklist: [],
      stakeholderIds: [],
      auto: { enabled: false, conditions: [], minSeverity: null },
      secretsSet: [],
      warRoomTypes: ['lark_app', 'slack_app', 'dingtalk_app', 'wecom_app'],
    }),
  );
  // No remediation action, cluster or ticket connector is set up here; a spec that needs them routes its own.
  app.get('/api/v1/alerts/remediation/actions', (_req, res) => res.json({ actions: [] }));
  app.get('/api/v1/alerts/remediation/clusters', (_req, res) => res.json({ clusters: [] }));
  app.get('/api/v1/alerts/incidents/:id/remediations', (_req, res) => res.json({ actions: [], runs: [] }));
  app.get('/api/v1/alerts/tickets/connectors', (_req, res) => res.json({ connectors: [] }));
  app.get('/api/v1/alerts/tickets/sites', (_req, res) =>
    res.json({
      meegle: [
        { key: 'feishu', url: 'https://project.feishu.cn' },
        { key: 'lark', url: 'https://meegle.com' },
      ],
    }),
  );
  app.get('/api/v1/alerts/incidents/:id/tickets', (_req, res) => res.json({ tickets: [] }));
  // Nobody here has asked for a swap or subscribed to a calendar; a spec that needs them routes its own.
  app.get('/api/v1/schedules/swaps', (_req, res) => res.json({ swaps: [] }));
  app.get('/api/v1/schedules/calendar-feeds', (_req, res) => res.json({ feeds: [] }));
  app.get('/api/v1/schedules/:id/swaps', (_req, res) => res.json({ swaps: [] }));
  app.get('/api/v1/schedules', (_req, res) => res.json(schedules(now).map((schedule) => withCoverage(schedule, now))));
  app.post('/api/v1/schedules/coverage-preview', (req, res) => {
    const draft = { ...schedules(now)[0]!, ...(req.body as Partial<Schedule> & { from?: number; to?: number }) };
    const from = Number(req.body?.from ?? now);
    res.json(scheduleCoverage(draft, from, Number(req.body?.to ?? from + 7 * DAY)));
  });
  app.get('/api/v1/schedules/:id/coverage', (req, res) => {
    const schedule = schedules(now).find((item) => item.id === req.params.id) ?? schedules(now)[0]!;
    const from = Number(req.query.from ?? now);
    res.json(scheduleCoverage(schedule, from, Number(req.query.to ?? from + 14 * DAY)));
  });
  app.get('/api/v1/schedules/:id', (req, res) =>
    res.json(withCoverage(schedules(now).find((schedule) => schedule.id === req.params.id) ?? schedules(now)[0]!, now)),
  );
  app.get('/api/v1/schedules/:id/on-call', (req, res) => res.json({ userId: req.params.id === 'sch-payments' ? 'u3' : 'u2' }));
}
