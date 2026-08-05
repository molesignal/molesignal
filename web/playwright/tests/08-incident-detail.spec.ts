/**
 * Incident detail — full-page deep link (`/alerts/incidents/:id`).
 *
 * Covers P1·F2: the route used to be a `PagePlaceholder`; it now renders
 * the real {@link IncidentDetail} page, which reuses the drawer's
 * `IncidentBody` renderer and wires ack / resolve against the existing
 * backend. This spec walks the bookmarkable / keyboard-nav path that the
 * drawer-based Flow 1 spec does not exercise:
 *
 *   navigate directly to /alerts/incidents/:id
 *     → header shows summary + severity / status
 *     → body surfaces timeline / cross-signal handles / triggering query
 *     → Acknowledge + Resolve mutate and refetch
 *     → a deleted / cross-org id reads as a clean "not found" empty
 *
 * Backend is mocked in-test via `page.route` (same isolation choice as
 * the Flow 1 spec) so the shared mock fixture stays untouched.
 */
import { expect, test } from '@playwright/test';

import { installMockShellSession } from '../fixtures/mockSession';

const NOW_MICROS = Date.UTC(2026, 4, 29, 9, 42, 0) * 1000;
const INCIDENT_ID = 'inc-detail';

const INCIDENT = {
  id: INCIDENT_ID,
  org_id: 'org-default',
  rule_id: 'rule-web-p95',
  status: 'open' as const,
  severity: 'critical' as const,
  summary: 'web service p95 > 500ms for 5m',
  fingerprint: 'abc123fingerprint',
  current_step: 1,
  current_step_started_at: NOW_MICROS,
  assignees: ['oncall@acme'],
  created_at: NOW_MICROS,
  labels: { severity: 'critical', service: 'web', env: 'production' },
  annotations: { runbook: 'https://runbooks.acme/web-latency' },
  trace_ids: ['trace-abc123def456789', 'trace-deadbeefcafe'],
  host_ids: ['node-3.us-east-1'],
  affected_services: ['web', 'api-gateway'],
  triggering_query: {
    language: 'sql' as const,
    statement:
      "SELECT percentile_cont(0.95) WITHIN GROUP (ORDER BY duration_ms) FROM traces WHERE service_name = 'web'",
    sample_values: [
      { ts: NOW_MICROS - 60_000_000, value: 612.4, labels: { service: 'web' } },
      { ts: NOW_MICROS, value: 802.1, labels: { service: 'web' } },
    ],
  },
};

test.describe('Incident detail — deep link', () => {
  test.beforeEach(async ({ page }) => {
    await installMockShellSession(page, {
      token: 'mock-incident-detail-token',
      orgId: INCIDENT.org_id,
      role: 'Admin',
      displayName: 'tester',
    });
    await page.addInitScript(() => {
      window.localStorage.setItem('molesignal-theme', 'dark');
      window.localStorage.setItem('molesignal-density', 'normal');
    });
  });

  test('renders full incident context and the back link', async ({ page }) => {
    await page.route(`**/api/v1/alerts/incidents/${INCIDENT_ID}`, (route) =>
      route.fulfill({ json: INCIDENT }),
    );

    await page.goto(`/alerts/incidents/${INCIDENT_ID}`);

    // Header title = summary.
    await expect(page.getByText(INCIDENT.summary)).toBeVisible({ timeout: 10_000 });
    // Metadata strip surfaces severity + status (scoped to the strip via
    // testid so we assert the actual Pills, not stray chrome / label text).
    await expect(page.getByTestId('incident-status')).toHaveText(INCIDENT.status);
    await expect(page.getByTestId('incident-severity')).toHaveText(INCIDENT.severity);
    // Body surfaces a cross-signal trace handle (SignalReference button).
    await expect(page.locator('[data-signal-type="trace_id"]').first()).toBeVisible();
    // Triggering query section renders the statement.
    await expect(page.getByText(/percentile_cont/i)).toBeVisible();
    // Back link points at the incidents list.
    await expect(page.getByRole('link', { name: /back/i })).toHaveAttribute(
      'href',
      '/alerts/incidents',
    );
  });

  test('Acknowledge then Resolve mutate, refetch, and re-gate the toolbar', async ({ page }) => {
    // Stateful detail mock: 'open' until /ack is POSTed, 'acknowledged'
    // after — so the post-mutation refetch genuinely drives the
    // status-gated toolbar (Ack hides, Resolve stays) rather than the mock
    // returning 'open' forever and hiding nothing.
    let status: 'open' | 'acknowledged' = 'open';
    let resolveCalled = false;
    await page.route(`**/api/v1/alerts/incidents/${INCIDENT_ID}`, (route) =>
      route.fulfill({ json: { ...INCIDENT, status } }),
    );
    await page.route(`**/api/v1/alerts/incidents/${INCIDENT_ID}/ack`, (route) => {
      status = 'acknowledged';
      return route.fulfill({ status: 200, json: {} });
    });
    await page.route(`**/api/v1/alerts/incidents/${INCIDENT_ID}/resolve`, (route) => {
      resolveCalled = true;
      return route.fulfill({ status: 200, json: {} });
    });

    await page.goto(`/alerts/incidents/${INCIDENT_ID}`);

    // Initially open → both Acknowledge and Resolve are offered.
    const ackBtn = page.getByTestId('incident-ack');
    const resolveBtn = page.getByTestId('incident-resolve');
    await expect(ackBtn).toBeVisible({ timeout: 10_000 });
    await expect(resolveBtn).toBeVisible();

    // Acknowledge → refetch flips status to 'acknowledged' → Ack hides,
    // Resolve remains (canResolve = open || acknowledged).
    await ackBtn.click();
    await expect(ackBtn).toHaveCount(0);
    await expect(resolveBtn).toBeVisible();
    await expect(page.getByTestId('incident-status')).toHaveText('acknowledged');

    // Resolve fires its POST.
    await resolveBtn.click();
    await expect.poll(() => resolveCalled, { timeout: 5_000 }).toBe(true);
  });

  test('a missing incident reads as a clean not-found empty state', async ({ page }) => {
    await page.route(`**/api/v1/alerts/incidents/${INCIDENT_ID}`, (route) =>
      route.fulfill({ status: 404, json: { error: 'not found' } }),
    );

    await page.goto(`/alerts/incidents/${INCIDENT_ID}`);

    await expect(page.getByText(/incident not found/i)).toBeVisible({ timeout: 10_000 });
    // No action buttons on a non-existent incident.
    await expect(page.getByTestId('incident-ack')).toHaveCount(0);
    await expect(page.getByTestId('incident-resolve')).toHaveCount(0);
  });
});
