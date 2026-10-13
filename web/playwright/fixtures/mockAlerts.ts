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
  MuteRule,
  Schedule,
  Severity,
} from '../../src/types/alerting';

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
  stream: { name: string; stream_type: 'logs' | 'metrics' | 'traces' };
  threshold: number;
  state: 'healthy' | 'pending' | 'firing';
  enabled?: boolean;
}

const RULE_SEEDS: RuleSeed[] = [
  { id: 'rule-api-5xx', name: 'Public API 5xx rate', description: 'Share of requests answered with a 5xx.', service: 'api-gateway', team: 'platform', severity: 'critical', language: 'promql', statement: 'sum(rate(http_requests_total{service="api-gateway",code=~"5.."}[5m])) / sum(rate(http_requests_total{service="api-gateway"}[5m])) * 100', stream: { name: 'http_requests_total', stream_type: 'metrics' }, threshold: 2, state: 'firing' },
  { id: 'rule-checkout-latency', name: 'Checkout p95 latency', description: 'p95 of the checkout transaction.', service: 'checkout', team: 'payments', severity: 'error', language: 'promql', statement: 'histogram_quantile(0.95, sum by (le) (rate(http_server_duration_bucket{service="checkout"}[5m])))', stream: { name: 'http_server_duration_bucket', stream_type: 'metrics' }, threshold: 800, state: 'firing' },
  { id: 'rule-dashboard-errors', name: 'Dashboard error logs', description: 'Errors logged by the web dashboard.', service: 'web-dashboard', team: 'frontend', severity: 'warning', language: 'sql', statement: "SELECT COUNT(*) FROM \"app_logs\" WHERE \"service.name\" = 'web-dashboard' AND level = 'ERROR'", stream: { name: 'app_logs', stream_type: 'logs' }, threshold: 50, state: 'firing' },
  { id: 'rule-ingest-lag', name: 'Ingestion lag', description: 'Seconds between receipt and searchability.', service: 'ingester', team: 'platform', severity: 'warning', language: 'promql', statement: 'max(ingest_lag_seconds)', stream: { name: 'ingest_lag_seconds', stream_type: 'metrics' }, threshold: 120, state: 'pending' },
  { id: 'rule-payments-declines', name: 'Payment declines', description: 'Unusual share of declined authorizations.', service: 'payments', team: 'payments', severity: 'error', kind: 'anomaly', language: 'promql', statement: 'sum(rate(payment_authorizations_total{result="declined"}[10m]))', stream: { name: 'payment_authorizations_total', stream_type: 'metrics' }, threshold: 0, state: 'healthy' },
  { id: 'rule-db-connections', name: 'Postgres connections', description: 'Connections in use on the primary.', service: 'postgres', team: 'platform', severity: 'warning', language: 'promql', statement: 'max(pg_stat_activity_count{role="primary"})', stream: { name: 'pg_stat_activity_count', stream_type: 'metrics' }, threshold: 380, state: 'healthy' },
  { id: 'rule-slow-traces', name: 'Slow order traces', description: 'Order traces slower than five seconds.', service: 'orders', team: 'orders', severity: 'warning', language: 'sql', statement: 'SELECT COUNT(DISTINCT trace_id) FROM "traces" WHERE "service.name" = \'orders\' AND duration_ns > 5000000000', stream: { name: 'traces', stream_type: 'traces' }, threshold: 20, state: 'healthy' },
  { id: 'rule-cert-expiry', name: 'Certificate expiry', description: 'Days left on the edge certificate.', service: 'edge', team: 'platform', severity: 'info', language: 'promql', statement: 'min(tls_certificate_days_remaining{host="app.acme.example"})', stream: { name: 'tls_certificate_days_remaining', stream_type: 'metrics' }, threshold: 21, state: 'healthy', enabled: false },
];

function rules(now: number): AlertRule[] {
  return RULE_SEEDS.map((seed, index) => ({
    id: seed.id,
    org_id: ORG,
    name: seed.name,
    description: seed.description,
    enabled: seed.enabled ?? true,
    kind: seed.kind ?? 'scheduled',
    query: { language: seed.language, statement: seed.statement, period_secs: 60, stream: seed.stream },
    trigger: { operator: seed.id === 'rule-cert-expiry' ? 'lt' : 'gt', threshold: seed.threshold, for_periods: 3, silence_secs: 600 },
    thresholds:
      seed.severity === 'critical'
        ? [
            { severity: 'warning', operator: 'gt', threshold: seed.threshold / 2, for_periods: 3 },
            { severity: 'critical', operator: 'gt', threshold: seed.threshold, for_periods: 3 },
          ]
        : [],
    severity: seed.severity,
    anomaly_params: seed.kind === 'anomaly' ? { algorithm: 'mad', lookback_days: 14, k: 3, weekly_seasonality: true } : null,
    escalation_policy_id: seed.team === 'payments' ? 'esc-payments' : 'esc-platform',
    labels: { service: seed.service, team: seed.team },
    annotations: { runbook: `https://runbooks.acme.example/${seed.id}` },
    last_eval_at: seed.enabled === false ? null : now - (index + 1) * 7_000_000,
    last_state: seed.state === 'pending' ? { kind: 'pending', consecutive: 2 } : { kind: seed.state },
    created_at: now - (120 - index * 9) * DAY,
    updated_at: now - (index + 2) * DAY,
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
    org_id: ORG,
    rule_id: seed.ruleId,
    escalation_policy_id: rule.team === 'payments' ? 'esc-payments' : 'esc-platform',
    status: seed.status,
    severity: seed.severity,
    summary: seed.summary,
    fingerprint: `${seed.ruleId}:${rule.service}`,
    current_step: seed.status === 'open' ? 1 : 0,
    current_loop: 0,
    current_step_started_at: createdAt + 5 * MINUTE,
    assignees: seed.acknowledged === undefined ? [] : ['u1'],
    created_at: createdAt,
    ...(seed.acknowledged === undefined ? {} : { acknowledged_at: now - seed.acknowledged, acknowledged_by: 'u1' }),
    ...(seed.resolved === undefined ? {} : { resolved_at: now - seed.resolved, resolved_by: 'u1' }),
    labels: { service: rule.service, team: rule.team, env: 'production', rule: rule.name },
    annotations: { runbook: `https://runbooks.acme.example/${rule.id}` },
    trace_ids: full ? ['7f3c9a1e5b2d4c6f8a0b1c2d3e4f5a6b', '1b2c3d4e5f60718293a4b5c6d7e8f901'] : ['7f3c9a1e5b2d4c6f8a0b1c2d3e4f5a6b'],
    host_ids: full ? ['edge-proxy-3', 'edge-proxy-7'] : ['edge-proxy-3'],
    affected_services: full ? [rule.service, 'web-dashboard'] : [rule.service],
    triggering_query: full ? { language: rule.language, statement: rule.statement, sample_values: samples } : null,
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
    window_secs: windowSecs,
    total: inWindow.length,
    active: inWindow.length - closed.length,
    closed: closed.length,
    mttr_secs: closed.length === 0 ? 0 : Math.round(closed.reduce((total, seed) => total + (seed.opened - seed.resolved!), 0) / closed.length / 1_000_000),
    noise_rate: 0.17,
    by_hour: byHour,
    by_severity: Object.fromEntries(count((seed) => seed.severity).map(({ key, count: total }) => [key, total])),
    top_services: count((seed) => RULE_SEEDS.find((rule) => rule.id === seed.ruleId)!.service),
    top_rules: count((seed) => seed.ruleId),
  };
}

const ESCALATIONS: EscalationPolicy[] = [
  {
    id: 'esc-platform',
    org_id: ORG,
    name: 'Platform on-call',
    steps: [
      { targets: [{ kind: 'schedule', schedule_id: 'sch-platform' }], ack_timeout_secs: 300, min_severity: null },
      { targets: [{ kind: 'user', user_id: 'u1' }], ack_timeout_secs: 600, min_severity: 'error' },
      { targets: [{ kind: 'team', team_id: 'team-platform' }], ack_timeout_secs: 900, min_severity: 'critical' },
    ],
    repeat: true,
    max_loops: 3,
  },
  {
    id: 'esc-payments',
    org_id: ORG,
    name: 'Payments',
    steps: [
      { targets: [{ kind: 'schedule', schedule_id: 'sch-payments' }], ack_timeout_secs: 300, min_severity: null },
      { targets: [{ kind: 'user', user_id: 'u1' }], ack_timeout_secs: 900, min_severity: 'error' },
    ],
    repeat: false,
    max_loops: 1,
  },
];

function schedules(now: number): Schedule[] {
  const schedule = (id: string, name: string, description: string, members: string[], overrides: Schedule['overrides'] = []): Schedule => ({
    id,
    org_id: ORG,
    name,
    description,
    timezone: 'UTC',
    enabled: true,
    rotations: [
      { id: `${id}-primary`, name: 'Primary', members, kind: 'weekly', active_window: null, start_at: now - 60 * DAY },
      { id: `${id}-daytime`, name: 'Business hours backup', members: [...members].reverse(), kind: 'daily', active_window: { weekday_mask: 0b0111110, hour_start: 9, hour_end: 18 }, start_at: now - 60 * DAY },
    ],
    overrides,
    created_by: 'u1',
    updated_by: 'u1',
    created_at: now - 60 * DAY,
    updated_at: now - 3 * DAY,
  });

  return [
    schedule('sch-platform', 'Platform on-call', 'Edge, ingestion and storage.', ['u1', 'u2', 'u3'], [
      { id: 'ovr-1', user_id: 'u2', start_at: now - 2 * HOUR, end_at: now + 6 * HOUR, reason: 'Covering while Dana is travelling' },
    ]),
    schedule('sch-payments', 'Payments on-call', 'Checkout and the payment providers.', ['u3', 'u1']),
  ];
}

function mutes(now: number): MuteRule[] {
  const mute = (id: string, name: string, comment: string, matchers: MuteRule['matchers'], window: MuteRule['window'], enabled = true): MuteRule => ({
    id,
    org_id: ORG,
    name,
    enabled,
    matchers,
    window,
    comment,
    created_by: 'u1',
    created_at: now - 20 * DAY,
    updated_at: now - 2 * DAY,
  });

  return [
    mute('mute-eu-upgrade', 'EU database upgrade', 'Planned upgrade of the EU primary.', [{ label: 'region', op: 'eq', value: 'eu-central-1' }], { type: 'fixed', start: now - 20 * MINUTE, end: now + 100 * MINUTE }),
    mute('mute-staging-nights', 'Staging at night', 'Staging is scaled down outside working hours.', [{ label: 'env', op: 'eq', value: 'staging' }], { type: 'recurring', timezone: 'UTC', weekday_mask: 0b1111111, hour_start: 20, hour_end: 6 }),
    mute('mute-load-test', 'Load test, March', 'Kept for reference.', [{ label: 'service', op: 're', value: 'checkout|payments' }], { type: 'fixed', start: now - 70 * DAY, end: now - 69 * DAY }, false),
  ];
}

function semanticGroups(now: number): SemanticGroup[] {
  return [
    { id: 'grp-edge', org_id: ORG, name: 'Edge and API', enabled: true, matchers: [{ label: 'team', op: 'eq', value: 'platform' }], group_by: ['service', 'env'], created_at: now - 40 * DAY, updated_at: now - 6 * DAY },
    { id: 'grp-payments', org_id: ORG, name: 'Money path', enabled: true, matchers: [{ label: 'service', op: 're', value: 'checkout|payments' }], group_by: ['service'], created_at: now - 30 * DAY, updated_at: now - 30 * DAY },
  ];
}

/** Registers the alert centre's endpoints on the mock backend. */
export function registerAlertRoutes(app: Express, nowIso: string): void {
  const now = Date.parse(nowIso) * 1_000;
  const seedOf = (id: string) => INCIDENT_SEEDS.find((seed) => seed.id === id) ?? INCIDENT_SEEDS[0]!;

  app.get('/api/v1/alerts/rules', (_req, res) => res.json(rules(now)));
  app.get('/api/v1/alerts/rules/:id', (req, res) =>
    res.json(rules(now).find((rule) => rule.id === req.params.id) ?? rules(now)[0]),
  );
  app.get('/api/v1/alerts/incidents', (req, res) => {
    const active = req.query.scope === 'active';
    const windowSecs = Number(req.query.window_secs ?? 0);
    res.json(
      INCIDENT_SEEDS.filter((seed) => !active || seed.resolved === undefined)
        .filter((seed) => !windowSecs || seed.opened <= windowSecs * 1_000_000)
        .map((seed) => incident(seed, now, false)),
    );
  });
  app.get('/api/v1/alerts/incidents/:id', (req, res) => res.json(incident(seedOf(req.params.id), now, true)));
  app.get('/api/v1/alerts/incidents/:id/rca', (_req, res) => res.status(404).json({ error: 'not_found', message: 'no analysis yet' }));
  app.get('/api/v1/alerts/insights', (req, res) => res.json(insights(now, Number(req.query.window_secs ?? 7 * 86_400))));
  app.get('/api/v1/alerts/escalations', (_req, res) => res.json(ESCALATIONS));
  app.get('/api/v1/alerts/escalations/:id', (req, res) =>
    res.json(ESCALATIONS.find((policy) => policy.id === req.params.id) ?? ESCALATIONS[0]),
  );
  app.get('/api/v1/alerts/mutes', (_req, res) => res.json(mutes(now)));
  app.get('/api/v1/alerts/semantic_groups', (_req, res) => res.json(semanticGroups(now)));
  app.get('/api/v1/schedules', (_req, res) => res.json(schedules(now)));
  app.get('/api/v1/schedules/:id', (req, res) =>
    res.json(schedules(now).find((schedule) => schedule.id === req.params.id) ?? schedules(now)[0]),
  );
  app.get('/api/v1/schedules/:id/on-call', (req, res) => res.json({ user_id: req.params.id === 'sch-payments' ? 'u3' : 'u2' }));
}
