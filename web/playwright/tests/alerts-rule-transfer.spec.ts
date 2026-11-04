/**
 * Rules as a file: a Prometheus rule file is tried, then imported at once; the rules in view are
 * exported as a bundle.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const PROMETHEUS = `groups:
  - name: api
    rules:
      - alert: HighErrorRate
        expr: sum(rate(http_requests_total{code=~"5.."}[5m])) > 1
        for: 5m
        labels:
          severity: critical
`;

test('a Prometheus rule file is tried, then imported at once', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const requests: Array<Record<string, unknown>> = [];
  await page.route(/\/api\/v1\/alerts\/rules\/import$/, (route) => {
    const body = route.request().postDataJSON() as Record<string, unknown>;
    requests.push(body);
    return route.fulfill({
      json: {
        format: 'prometheus',
        items: [{ name: 'HighErrorRate', action: 'create', ruleId: 'rule-new' }],
        notes: [],
        applied: body.dryRun === false,
      },
    });
  });

  await page.goto('/alerts/rules');
  await page.getByTestId('rule-transfer').click();
  await page.getByTestId('rule-import').click();

  const drawer = page.getByRole('dialog', { name: 'Import rules' });
  await drawer.getByRole('textbox', { name: 'Content' }).fill(PROMETHEUS);
  await expect(page.getByTestId('rule-import-apply')).toBeDisabled();
  await page.getByTestId('rule-import-try').click();

  await expect(drawer.locator('[data-rule-import-report="prometheus"]')).toContainText('HighErrorRate');
  await expect(drawer.locator('[data-import-item="create"]')).toHaveCount(1);
  await page.getByTestId('rule-import-apply').click();

  await expect.poll(() => requests.length).toBe(2);
  expect(requests[0]).toEqual({ content: PROMETHEUS, dryRun: true, createOnly: false });
  expect(requests[1]).toEqual({ content: PROMETHEUS, dryRun: false, createOnly: false });
  await expect(page.getByText('1 rule imported', { exact: true })).toBeVisible();
});

test('the rules in view are exported as a bundle', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let asked: string | null = null;
  await page.route(/\/api\/v1\/alerts\/rules\/export(\?.*)?$/, (route) => {
    asked = new URL(route.request().url()).searchParams.get('ids');
    return route.fulfill({ json: { format: 'molesignal.alert-rules/v1', rules: [{ name: 'a' }, { name: 'b' }] } });
  });

  await page.goto('/alerts/rules');
  const download = page.waitForEvent('download');
  await page.getByTestId('rule-transfer').click();
  await page.getByTestId('rule-export').click();

  expect((await download).suggestedFilename()).toMatch(/^molesignal-alert-rules-\d{4}-\d{2}-\d{2}\.json$/);
  // Every rule is in view, so the whole set is asked for.
  expect(asked).toBeNull();
});
