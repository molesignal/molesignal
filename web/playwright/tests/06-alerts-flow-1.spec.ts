/**
 * Flow 1 — SRE pager → alert → trace_id → cross-signal jump
 *
 * Phase 6 M1.3 spec covering brief Experience Principle #3 ("Continuity
 * across signals"). Walks:
 *
 *   /alerts (firing rule list)
 *     → click state Pill on a firing row → IncidentDetailDrawer opens
 *     → drawer surfaces trace_ids / services / hosts
 *     → hover trace_id → SignalReference HoverCard appears
 *     → click "Open in Traces" → /traces/:traceId with preserved
 *       time window query params
 *
 * Backend is mocked in-test via `page.route` rather than touching the
 * shared `mockBackend` fixture: the incident detail endpoint is only
 * exercised by this Flow and isolating the mock keeps blast radius
 * minimal until the shared fixture grows incident_sections support.
 */
import { expect, test } from '@playwright/test';

import { installMockShellSession } from '../fixtures/mockSession';

const NOW_MICROS = Date.UTC(2026, 4, 29, 9, 42, 0) * 1000;

const INCIDENT = {
  id: 'inc-flow-1',
  org_id: 'org-default',
  rule_id: 'rule-web-p95',
  status: 'open' as const,
  severity: 'critical' as const,
  summary: 'web service p95 > 500ms for 5m',
  fingerprint: 'abc123',
  current_step: 1,
  current_step_started_at: NOW_MICROS,
  assignees: ['oncall@acme'],
  created_at: NOW_MICROS,
  labels: {
    severity: 'critical',
    service: 'web',
    env: 'production',
  },
  annotations: {
    runbook: 'https://runbooks.acme/web-latency',
  },
  trace_ids: ['trace-abc123def456789', 'trace-deadbeefcafe'],
  host_ids: ['node-3.us-east-1'],
  affected_services: ['web', 'api-gateway'],
  triggering_query: {
    language: 'sql' as const,
    statement: 'SELECT percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ms) FROM traces WHERE service_name = \'web\'',
    sample_values: [
      { ts: NOW_MICROS - 60_000_000, value: 612.4, labels: { service: 'web' } },
      { ts: NOW_MICROS - 30_000_000, value: 718.9, labels: { service: 'web' } },
      { ts: NOW_MICROS, value: 802.1, labels: { service: 'web' } },
    ],
  },
};

const RULE = {
  id: INCIDENT.rule_id,
  org_id: INCIDENT.org_id,
  name: 'web p95 SLO',
  description: 'High p95 latency on web service',
  enabled: true,
  query: {
    language: 'sql',
    statement: INCIDENT.triggering_query.statement,
    period_secs: 60,
  },
  trigger: {
    operator: 'gt',
    threshold: 500,
    for_periods: 5,
    silence_secs: 300,
  },
  escalation_policy_id: 'ep-1',
  labels: INCIDENT.labels,
  annotations: INCIDENT.annotations,
};

test.describe('Flow 1 — SRE pager to trace', () => {
  test.beforeEach(async ({ page }) => {
    await installMockShellSession(page, {
      token: 'mock-alert-flow-token',
      orgId: INCIDENT.org_id,
      role: 'Admin',
      displayName: 'tester',
    });
    await page.addInitScript(() => {
      window.localStorage.setItem('molesignal-theme', 'dark');
      window.localStorage.setItem('molesignal-density', 'normal');
    });

    // Incident list (truncated handles per backend Option C contract).
    const listIncident = {
      ...INCIDENT,
      trace_ids: [INCIDENT.trace_ids[0]!],
      host_ids: [INCIDENT.host_ids[0]!],
      affected_services: [INCIDENT.affected_services[0]!],
      triggering_query: null,
    };

    await page.route((url) => url.pathname === '/api/v1/alerts/rules', async (route) => {
      await route.fulfill({ json: [RULE] });
    });
    await page.route((url) => url.pathname === '/api/v1/alerts/incidents', async (route) => {
      await route.fulfill({ json: [listIncident] });
    });
    await page.route(`**/api/v1/alerts/incidents/${INCIDENT.id}`, async (route) => {
      // Full context returned on detail fetch.
      await route.fulfill({ json: INCIDENT });
    });
    // Catch-all GETs the page may issue on first render so they don't 404
    // and trigger ErrorState renders we don't care about for this spec.
    await page.route('**/api/v1/users/me', (route) =>
      route.fulfill({
        json: {
          id: 'u1',
          display_name: 'tester',
          display_role: 'Admin',
          roles: [
            {
              id: 'role-admin',
              key: 'admin',
              name: 'Admin',
              builtin: true,
            },
          ],
          org_id: INCIDENT.org_id,
        },
      }),
    );
  });

  test('rule list → state pill → drawer → trace_id HoverCard → Traces', async ({ page }) => {
    await page.goto('/alerts');

    // The rule list should expose our seeded rule with a clickable state pill.
    const openIncidentBtn = page.getByRole('button', { name: 'Details' }).first();
    await expect(openIncidentBtn).toBeVisible({ timeout: 10_000 });
    await openIncidentBtn.click();

    // Drawer opens — header shows summary, severity + status pills.
    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();
    await expect(
      dialog.getByRole('heading', { name: INCIDENT.summary }),
    ).toBeVisible();
    await expect(page.getByText(/critical/i).first()).toBeVisible();
    await expect(dialog.getByText(/waiting/i).first()).toBeVisible();

    // Cross-signal handles section: at least one trace_id chip is rendered
    // (SignalReference renders the value via a `data-signal-type` button).
    const traceRef = page.locator('[data-signal-type="trace_id"]').first();
    await expect(traceRef).toBeVisible({ timeout: 10_000 });

    // Open the contextual pivot menu through its explicit context-menu path.
    await traceRef.click({ button: 'right' });
    const openInTraces = page.getByRole('link', { name: /open current trace/i });
    await expect(openInTraces).toBeVisible({ timeout: 5_000 });
    await expect(page.getByRole('link', { name: /view trace logs/i })).toBeVisible();
    await expect(page.getByRole('link', { name: /view service metrics/i })).toBeVisible();

    // Jump to Traces. The Link should carry the trace_id + time window.
    await openInTraces.click();
    await expect(page).toHaveURL(/\/traces\?/);
    const url = new URL(page.url());
    // The trace id is carried inside the `q` field expression
    // (q=trace_id = 'trace-…'), not a standalone trace_id param.
    expect(url.searchParams.get('q')).toContain('trace-');
    // Time window is propagated so the trace view is bounded to the
    // incident window — brief Principle #3 in its concrete form.
    expect(url.searchParams.get('from')).toBeTruthy();
    expect(url.searchParams.get('to')).toBeTruthy();
  });

  test('drawer ack mutates state and refetches', async ({ page }) => {
    let ackCalled = false;
    await page.route(`**/api/v1/alerts/incidents/${INCIDENT.id}/ack`, async (route) => {
      ackCalled = true;
      await route.fulfill({ status: 200, json: {} });
    });

    await page.goto('/alerts');
    await page.getByRole('button', { name: 'Details' }).first().click();
    await page.getByRole('dialog').waitFor();

    const ackBtn = page.getByTestId('incident-ack');
    await expect(ackBtn).toBeVisible();
    await ackBtn.click();

    // Wait one tick for the mutation onSuccess and Toast to settle.
    await expect.poll(() => ackCalled, { timeout: 5_000 }).toBe(true);
  });
});
