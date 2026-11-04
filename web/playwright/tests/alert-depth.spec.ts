/** Rule backtest, incident timeline, on-call coverage and deliveries by alert. */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('the rule workbench', () => {
  test('says what the rule would have done over the stretch it tested', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    const start = Date.parse('2026-05-23T09:00:00Z') * 1000;
    const rows = Array.from({ length: 20 }, (_, index) => [start + index * 60_000_000, index % 7 === 3 ? 5000 : 1]);
    await page.route('**/api/v1/query**', (route) =>
      route.fulfill({ json: { columns: ['_timestamp', 'value'], rows, scannedRows: 20, tookMs: 3 } }),
    );

    await page.goto('/alerts/rules/new');
    await page.getByPlaceholder('e.g. app_metrics', { exact: true }).fill('app_metrics');
    await page.getByRole('button', { name: 'Test rule', exact: true }).click();

    const summary = page.locator('[data-rule-backtest]');
    await expect(summary).toBeVisible({ timeout: 10_000 });
    await expect(summary).toContainText(/would (not )?have fired/);
  });
});

test.describe('the incident timeline', () => {
  test('gives its phases and narrows by what happened', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    const now = Date.parse('2026-05-23T10:00:00Z') * 1000;
    await page.route('**/api/v1/alerts/incidents/*/timeline', (route) =>
      route.fulfill({
        json: {
          entries: [
            { id: 'e1', orgId: 'o', incidentId: 'inc-2291', at: now - 1_000_000_000, kind: 'acknowledged', actor: 'u-1', detail: {} },
            { id: 'e2', orgId: 'o', incidentId: 'inc-2291', at: now - 900_000_000, kind: 'note', actor: 'u-1', detail: { text: 'Looking at the gateway' } },
          ],
          deliveries: [],
          merged: [],
        },
      }),
    );

    await page.goto('/alerts/incidents/inc-2291');

    const phases = page.locator('[data-incident-phases]');
    await expect(phases).toContainText('Acknowledged after');
    await expect(phases).toContainText('Resolved after');

    const rows = page.locator('[data-timeline-row]');
    await expect(rows).toHaveCount(3);
    await page.getByRole('button', { name: /^Notes/ }).click();
    await expect(rows).toHaveCount(1);
    await expect(rows).toContainText('Looking at the gateway');
  });
});

test.describe('the on-call schedule', () => {
  test('shows two weeks hour by hour and says how it stands', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/alerts/schedules');
    await page.getByRole('row', { name: /Platform on-call/ }).click();

    await expect(page.locator('[data-coverage-grid]')).toBeVisible();
    await expect(page.locator('[data-coverage-cell]')).toHaveCount(14 * 24);
    await expect(page.locator('[data-coverage-summary]')).toContainText(/next 14 days/);
  });
});

test.describe('deliveries', () => {
  test('gather by alert, each try a mark along its channel', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    const now = Date.parse('2026-05-23T09:30:00Z') * 1000;
    const delivery = (id: string, attempt: number, status: string, connector: string, created: number) => ({
      id,
      organizationId: 'o',
      eventId: 'alert.triggered:inc-1',
      connectorId: connector,
      targetType: 'direct_user',
      stage: 'initial',
      attempt,
      status,
      idempotencyKey: id,
      createdAt: created,
      sentAt: created + 1_000_000,
    });
    await page.route('**/api/v1/notify/deliveries**', (route) =>
      route.fulfill({
        json: [
          delivery('a', 1, 'failed', 'c-email', now),
          delivery('b', 2, 'success', 'c-email', now + 30_000_000),
          delivery('c', 1, 'success', 'c-pager', now + 2_000_000),
        ],
      }),
    );

    await page.goto('/settings/notify/deliveries');

    await expect(page.locator('table tbody tr')).toHaveCount(1);
    await expect(page.locator('[data-delivery-mark]')).toHaveCount(3);
    await expect(page.locator('[data-delivery-mark="failed"]')).toHaveCount(1);

    await page.getByRole('button', { name: 'By attempt' }).click();
    await expect(page.locator('table tbody tr')).toHaveCount(3);
  });

  test('a policy an alert pipeline kept from sending says why, as a skip and not a failure', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    const created = Date.parse('2026-05-23T09:30:00Z') * 1000;
    await page.route('**/api/v1/notify/deliveries**', (route) =>
      route.fulfill({
        json: [
          {
            id: 'skipped-1',
            organizationId: 'o',
            eventId: 'alert.triggered:inc-1',
            policyId: 'policy-1',
            targetType: 'policy',
            stage: 'user_primary',
            attempt: 1,
            status: 'skipped',
            errorCode: 'dropped_by_pipeline',
            errorMessage: 'dropped by alert pipeline "quiet", step "night"',
            idempotencyKey: 'skipped-1',
            createdAt: created,
            sentAt: created,
          },
        ],
      }),
    );

    await page.goto('/settings/notify/deliveries');
    await page.getByRole('button', { name: 'By attempt' }).click();
    await page.locator('table tbody tr').first().click();

    const chain = page.getByRole('dialog');
    await expect(chain.getByText('Reason', { exact: true })).toBeVisible();
    await expect(chain).toContainText('dropped by alert pipeline "quiet", step "night"');
    await expect(chain.getByText('Failure', { exact: true })).toHaveCount(0);
    await expect(chain.getByRole('button', { name: 'Retry failed routes' })).toBeDisabled();
  });
});
