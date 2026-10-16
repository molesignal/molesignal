/**
 * Mock business services of "Acme Cloud", consistent with the monitors, alert
 * rules and status page components of the reliability mocks: the public API is
 * in a partial outage because its health check is failing, and checkout,
 * which leans on it, is degraded.
 *
 * Every time is relative to the mock session's frozen clock.
 */
import type { Express } from 'express';

import type { BusinessService, BusinessServiceHealthPeriod } from '../../src/api/businessServices';

const ORG = 'acme-prod';
const MINUTE = 60_000_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

function history(
  now: number,
  periods: Array<[BusinessServiceHealthPeriod['status'], number]>,
): BusinessServiceHealthPeriod[] {
  // Each entry is [status, started this long ago]; each period ends where the next begins.
  return periods.map(([status, ago], index) => ({
    status,
    started_at: now - ago,
    ended_at: index + 1 < periods.length ? now - periods[index + 1]![1] : null,
  }));
}

export function businessServices(now: number): BusinessService[] {
  const base = {
    org_id: ORG,
    created_by: 'user-alex',
    created_at: now - 40 * DAY,
    updated_at: now - 2 * DAY,
  };

  return [
    {
      ...base,
      id: 'bs-public-api',
      name: 'Public API',
      description: 'What integrations and SDKs call at api.acme.example.',
      dependencies: [
        { kind: 'technical_service', target: 'api-gateway', impact: 'outage' },
        { kind: 'synthetic_monitor', target: 'mon-checkout-api', impact: 'outage' },
        { kind: 'alert_rule', target: 'rule-api-5xx', impact: 'outage' },
      ],
      health: {
        status: 'partial_outage',
        causes: [
          {
            kind: 'alert_rule',
            target: 'rule-api-5xx',
            status: 'major_outage',
            incidents: [{ id: 'inc-api-5xx', summary: 'Public API 5xx rate above 2%', severity: 'critical' }],
            incident_count: 1,
          },
          { kind: 'synthetic_monitor', target: 'mon-checkout-api', status: 'major_outage', monitor_state: 'failing' },
        ],
        changed_at: now - 38 * MINUTE,
        evaluated_at: now - MINUTE,
        history: history(now, [
          ['operational', 8 * DAY],
          ['degraded_performance', 3 * DAY + 2 * HOUR],
          ['operational', 3 * DAY],
          ['partial_outage', 38 * MINUTE],
        ]),
      },
      linked_components: [
        {
          status_page_id: 'sp-acme-cloud',
          status_page_name: 'Acme Cloud',
          component_id: 'cmp-api',
          component_name: 'Public API',
          component_status: 'partial_outage',
          mode: 'follow',
          notify_subscribers: true,
        },
      ],
    },
    {
      ...base,
      id: 'bs-checkout',
      name: 'Checkout',
      description: 'Plan changes and card payments in the dashboard.',
      dependencies: [
        { kind: 'business_service', target: 'bs-public-api', impact: 'degraded' },
        { kind: 'synthetic_monitor', target: 'mon-payments-gateway', impact: 'outage' },
        { kind: 'alert_rule', target: 'rule-checkout-latency', impact: 'degraded' },
      ],
      health: {
        status: 'degraded_performance',
        causes: [
          { kind: 'business_service', target: 'bs-public-api', status: 'degraded_performance' },
          {
            kind: 'alert_rule',
            target: 'rule-checkout-latency',
            status: 'degraded_performance',
            incidents: [{ id: 'inc-checkout-latency', summary: 'Checkout p95 latency above 800 ms', severity: 'error' }],
            incident_count: 1,
          },
        ],
        changed_at: now - 36 * MINUTE,
        evaluated_at: now - MINUTE,
        history: history(now, [
          ['operational', 8 * DAY],
          ['degraded_performance', 36 * MINUTE],
        ]),
      },
      linked_components: [
        {
          status_page_id: 'sp-acme-cloud',
          status_page_name: 'Acme Cloud',
          component_id: 'cmp-payments',
          component_name: 'Billing and payments',
          component_status: 'operational',
          mode: 'automation',
          notify_subscribers: true,
        },
      ],
    },
    {
      ...base,
      id: 'bs-sign-in',
      name: 'Sign-in',
      description: 'Password, SSO and session renewal.',
      dependencies: [{ kind: 'synthetic_monitor', target: 'mon-login-journey', impact: 'outage' }],
      health: {
        status: 'operational',
        causes: [],
        changed_at: now - 12 * DAY,
        evaluated_at: now - MINUTE,
        history: history(now, [['operational', 12 * DAY]]),
      },
      linked_components: [],
    },
  ];
}

/** Registers the business service endpoints on the mock backend. */
export function registerBusinessServiceRoutes(app: Express, nowIso: string): void {
  const now = Date.parse(nowIso) * 1_000;

  app.get('/api/v1/business-services', (_req, res) => res.json(businessServices(now)));
  app.get('/api/v1/business-services/:id', (req, res) => {
    const service = businessServices(now).find((item) => item.id === req.params.id);
    if (!service) return res.status(404).json({ error: 'business service not found' });
    return res.json(service);
  });
}
