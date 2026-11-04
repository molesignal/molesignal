/**
 * What the server answers about alert rules on RUM figures: the preview of a figure over the last
 * stretch of time, and the rules the pages create, listed back so that a page sees the rules it made.
 * The numbers are made up to be worth drawing (a figure that climbs through its line near the end, a
 * few windows with too few rows at the start); what the real figures are is checked against a real
 * node, not here.
 *
 * `installRumAlertCenter` adds what the pages of RUM read of the alert centre: the rules of an
 * application, the incidents going on with the sessions they kept, and the figures the templates show
 * as they are now.
 */
import type { Page } from '@playwright/test';

import type { RumAlertPreview } from '../../src/api/rum';
import { rumDefaultMinSamples, rumMetricUnit } from '../../src/routes/alerts/rule-workbench/rumSource';
import type { AlertRule, Incident, RumAlertMetric, RumAlertQuery, Severity } from '../../src/types/alerting';
import { FROZEN_NOW_ISO } from './mockBackend';
import { MOCK_RUM } from './mockRumData';

const MINUTE = 60_000_000;
const HOUR = 3_600_000_000;

/** Where a figure moves between, and to how many decimals it is told. */
const SHAPES: Record<RumAlertMetric, { low: number; high: number; digits: number }> = {
  session_count: { low: 180, high: 640, digits: 0 },
  view_count: { low: 520, high: 1_900, digits: 0 },
  error_count: { low: 2, high: 46, digits: 0 },
  error_rate: { low: 0.6, high: 3.4, digits: 1 },
  crash_free_sessions: { low: 99.95, high: 99.1, digits: 2 },
  frustration_rate: { low: 0.5, high: 4.2, digits: 1 },
  failed_request_rate: { low: 0.4, high: 2.9, digits: 1 },
  lcp_p75: { low: 1_900, high: 3_400, digits: 0 },
  inp_p75: { low: 140, high: 330, digits: 0 },
  cls_p75: { low: 0.06, high: 0.14, digits: 2 },
  ttfb_p75: { low: 480, high: 1_150, digits: 0 },
  new_issues: { low: 0, high: 3, digits: 0 },
  regressed_issues: { low: 0, high: 2, digits: 0 },
  issue_impact: { low: 4, high: 60, digits: 0 },
  custom_timing: { low: 300, high: 900, digits: 0 },
  custom_vital: { low: 800, high: 2_400, digits: 0 },
  query: { low: 10, high: 120, digits: 0 },
};

/** The windows a preview holds when the request names no range: as the server chooses them. */
function windowsOf(periodSecs: number): number {
  const bar = periodSecs * 1_000_000;
  const wanted = Math.min(72, Math.max(12, Math.floor((6 * HOUR) / bar)));

  return Math.min(wanted, Math.max(1, Math.floor((24 * HOUR) / bar)));
}

/** What a figure is now, as the preview of one window says: a number, or none where too few rows are. */
export type CurrentFigures = Partial<Record<RumAlertMetric, number | null>>;

export function previewFor(
  query: RumAlertQuery,
  periodSecs: number,
  now: number = Date.now() * 1_000,
  current?: number | null,
): RumAlertPreview {
  const bar = periodSecs * 1_000_000;
  const windows = windowsOf(periodSecs);
  const from = now - windows * bar;
  const shape = SHAPES[query.metric];
  const minimum = query.minSamples ?? rumDefaultMinSamples(query.metric);
  const scale = 10 ** shape.digits;

  const points: RumAlertPreview['points'] = Array.from({ length: windows }, (_, index) => {
    const at = from + index * bar;

    // The first windows are too quiet for a figure made of rows to say anything.
    if (index < 2 && minimum > 0) return { at, value: null, samples: null };

    const wave = 0.5 + 0.5 * Math.sin(index / 3);
    const climb = index >= windows - 4 ? (index - (windows - 5)) / 4 : 0;
    const level = Math.min(1, wave * 0.6 + climb * 0.4);
    const value = Math.round((shape.low + (shape.high - shape.low) * level) * scale) / scale;

    return { at, value, samples: minimum > 0 ? minimum * 2 + 30 : Math.max(0, Math.round(value)) };
  });

  // A figure the test names is what the last window says.
  if (current !== undefined && points.length > 0) {
    const last = points.length - 1;

    points[last] = { at: points[last]!.at, value: current, samples: current === null ? null : minimum > 0 ? minimum * 2 + 30 : Math.round(current) };
  }

  return {
    metric: query.metric,
    unit: rumMetricUnit(query.metric),
    minSamples: minimum,
    periodSecs: periodSecs,
    from,
    to: now,
    points,
    current: points.at(-1) ?? null,
  };
}

export interface RumAlertsCaptured {
  /** The rules created, as sent. */
  rules: Array<Record<string, unknown>>;
  /** The previews asked for, as sent. */
  previews: Array<{ query: RumAlertQuery; periodSecs: number }>;
}

/** Answers the preview and keeps the rules the page creates and updates. */
export async function installRumAlertRoutes(page: Page, current: CurrentFigures = {}): Promise<RumAlertsCaptured> {
  const captured: RumAlertsCaptured = { rules: [], previews: [] };

  await page.route(/\/api\/v1\/rum\/alerts\/preview$/, async (route) => {
    const body = route.request().postDataJSON() as { query: RumAlertQuery; periodSecs: number; to?: number };

    captured.previews.push(body);

    return route.fulfill({
      json: previewFor(body.query, body.periodSecs, body.to, Object.hasOwn(current, body.query.metric) ? current[body.query.metric] : undefined),
    });
  });

  await page.route(/\/api\/v1\/alerts\/rules$/, async (route) => {
    if (route.request().method() !== 'POST') return route.fallback();

    const body = route.request().postDataJSON() as Record<string, unknown>;

    captured.rules.push(body);

    return route.fulfill({
      json: { ...body, id: `rule-${captured.rules.length}`, orgId: 'default', escalationPolicyId: 'default' },
    });
  });

  return captured;
}

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;

/** A rule on a RUM figure, as the server lists it. */
export function rumRule(rule: {
  id: string;
  name: string;
  metric: RumAlertMetric;
  application?: string;
  /** None is every environment. */
  environment?: string | null;
  page?: string | null;
  state?: 'healthy' | 'pending' | 'firing';
  enabled?: boolean;
  /** The template it was made from. */
  template?: string;
  period?: number;
  bands?: ReadonlyArray<{ severity: Severity; operator: 'gt' | 'lt'; threshold: number }>;
}): AlertRule {
  const application = rule.application ?? MOCK_RUM.applications.web;
  const environment = rule.environment === undefined ? 'prod' : rule.environment;
  const bands = rule.bands ?? [
    { severity: 'warning' as const, operator: 'gt' as const, threshold: 2 },
    { severity: 'critical' as const, operator: 'gt' as const, threshold: 5 },
  ];

  return {
    id: rule.id,
    orgId: 'acme-prod',
    name: rule.name,
    description: '',
    enabled: rule.enabled ?? true,
    kind: 'scheduled',
    query: {
      language: 'sql',
      statement: `rum:${rule.metric} ${application}`,
      periodSecs: rule.period ?? 900,
      stream: null,
      rum: { applicationId: application, environment, page: rule.page ?? null, version: null, metric: rule.metric },
    },
    trigger: { operator: bands[0]!.operator, threshold: bands[0]!.threshold, forPeriods: 1, silenceSecs: 300 },
    thresholds: bands.map((band) => ({ ...band, forPeriods: 1 })),
    severity: null,
    escalationPolicyId: 'default',
    labels: {
      'rum.application': application,
      ...(environment ? { 'deployment.environment': environment } : {}),
      ...(rule.page ? { 'rum.page': rule.page } : {}),
      ...(rule.template ? { 'rum.template': rule.template } : {}),
    },
    annotations: {},
    lastEvalAt: NOW - 20_000_000,
    lastState: rule.state === 'firing' ? { kind: 'firing' } : rule.state === 'pending' ? { kind: 'pending', consecutive: 1 } : { kind: 'healthy' },
    createdAt: NOW - 10 * HOUR,
    updatedAt: NOW - HOUR,
  };
}

/** An incident of a rule on a RUM figure, as the incident drawer reads it: with the sessions it kept. */
export function rumIncident(incident: {
  id: string;
  rule: AlertRule;
  summary: string;
  severity: Severity;
  status?: Incident['status'];
  /** Minutes before now. */
  opened: number;
  /** The sessions it kept, with the figure in each and what went with it. */
  samples?: ReadonlyArray<{ session: string; minutes: number; value?: number; detail?: string }>;
}): Incident {
  const created = NOW - incident.opened * MINUTE;
  const figure = incident.rule.query.rum!;

  return {
    id: incident.id,
    orgId: 'acme-prod',
    ruleId: incident.rule.id,
    escalationPolicyId: 'default',
    status: incident.status ?? 'open',
    severity: incident.severity,
    summary: incident.summary,
    fingerprint: `${incident.rule.id}:${figure.applicationId}`,
    currentStep: 1,
    currentLoop: 0,
    currentStepStartedAt: created + 5 * MINUTE,
    assignees: [],
    createdAt: created,
    labels: incident.rule.labels,
    annotations: {},
    traceIds: [],
    hostIds: [],
    affectedServices: [],
    triggeringQuery: {
      language: 'sql',
      statement: incident.rule.query.statement,
      sampleValues: (incident.samples ?? []).map((sample) => ({
        ts: NOW - sample.minutes * MINUTE,
        value: 4.3,
        labels: {
          value: '4.3',
          application: figure.applicationId,
          'rum.session_id': sample.session,
          ...(sample.value === undefined ? {} : { 'rum.value': String(sample.value) }),
          ...(sample.detail ? { 'rum.detail': sample.detail } : {}),
        },
      })),
    },
  };
}

/** What the alert centre holds when a page of RUM opens. */
export interface RumAlertCenter {
  rules: AlertRule[];
  incidents: Incident[];
  /** What the figures are now, by the figure; the others follow the made-up curve. */
  current?: CurrentFigures;
}

/** The rules and incidents of the standard story: a slow checkout page going on in production. */
export function standardCenter(): RumAlertCenter {
  const slow = rumRule({
    id: 'rule-rum-lcp',
    name: '/checkout page loads slowly',
    metric: 'lcp_p75',
    page: '/checkout',
    state: 'firing',
    period: 3_600,
    bands: [
      { severity: 'warning', operator: 'gt', threshold: 2_500 },
      { severity: 'critical', operator: 'gt', threshold: 4_000 },
    ],
  });

  return {
    rules: [
      slow,
      rumRule({ id: 'rule-rum-errors', name: 'Storefront · prod · Page errors', metric: 'error_rate', template: 'error_rate' }),
      rumRule({ id: 'rule-rum-staging-cls', name: 'Staging layout shifts', metric: 'cls_p75', environment: 'staging', period: 3_600 }),
      rumRule({ id: 'rule-rum-frustration', name: 'Frustrated sessions anywhere', metric: 'frustration_rate', environment: null, enabled: false, template: 'frustration', period: 3_600 }),
      rumRule({ id: 'rule-rum-other', name: 'Checkout web errors', metric: 'error_rate', application: 'checkout-web' }),
    ],
    incidents: [
      rumIncident({
        id: 'inc-rum-lcp',
        rule: slow,
        summary: 'Storefront · /checkout page loads slowly',
        severity: 'critical',
        opened: 12,
        samples: [
          { session: MOCK_RUM.session.checkout, minutes: 14, value: 7_213.5, detail: 'img.hero' },
          { session: MOCK_RUM.session.anonymous, minutes: 16, value: 6_402.1, detail: 'div.order-summary' },
          { session: MOCK_RUM.session.frustrated, minutes: 19, value: 5_811 },
        ],
      }),
      rumIncident({
        id: 'inc-rum-other',
        rule: rumRule({ id: 'rule-rum-other', name: 'Checkout web errors', metric: 'error_rate', application: 'checkout-web' }),
        summary: 'Checkout web error rate',
        severity: 'warning',
        opened: 40,
      }),
    ],
    current: { lcp_p75: 2_400, inp_p75: 180, cls_p75: 0.13, error_rate: 1.8, failed_request_rate: 1.2, frustration_rate: null, new_issues: 0, regressed_issues: 0, session_count: 38 },
  };
}

/**
 * Answers what a page of RUM reads of the alert centre: the rules (the ones made from a template are
 * listed back), the incidents going on and each one's detail, and the figures as they are now.
 */
export async function installRumAlertCenter(page: Page, center: RumAlertCenter = standardCenter()): Promise<RumAlertsCaptured & { stored: AlertRule[] }> {
  const captured = await installRumAlertRoutes(page, center.current ?? {});
  const stored = [...center.rules];

  await page.route(/\/api\/v1\/alerts\/rules$/, async (route) => {
    if (route.request().method() === 'GET') return route.fulfill({ json: stored });
    if (route.request().method() !== 'POST') return route.fallback();

    const body = route.request().postDataJSON() as Record<string, unknown>;
    const made = { ...body, id: `rule-made-${captured.rules.length + 1}`, orgId: 'acme-prod', escalationPolicyId: 'default', lastState: { kind: 'healthy' }, createdAt: NOW, updatedAt: NOW } as unknown as AlertRule;

    captured.rules.push(body);
    stored.push(made);

    return route.fulfill({ json: made });
  });

  await page.route(/\/api\/v1\/alerts\/rules\/[^/?]+$/, (route) => {
    const id = decodeURIComponent(new URL(route.request().url()).pathname.split('/').pop() ?? '');
    const found = stored.find((rule) => rule.id === id);

    return found && route.request().method() === 'GET' ? route.fulfill({ json: found }) : route.fallback();
  });

  await page.route(/\/api\/v1\/alerts\/incidents(\?.*)?$/, (route) => {
    const active = new URL(route.request().url()).searchParams.get('scope') === 'active';

    return route.fulfill({
      json: center.incidents
        .filter((incident) => !active || incident.status === 'open' || incident.status === 'acknowledged')
        // The list of incidents does not carry the query that opened one.
        .map((incident) => ({ ...incident, triggeringQuery: null })),
    });
  });

  await page.route(/\/api\/v1\/alerts\/incidents\/[^/?]+$/, (route) => {
    const id = decodeURIComponent(new URL(route.request().url()).pathname.split('/').pop() ?? '');
    const found = center.incidents.find((incident) => incident.id === id);

    return found ? route.fulfill({ json: found }) : route.fallback();
  });

  return { ...captured, stored };
}
