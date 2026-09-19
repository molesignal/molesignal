import { expect, test } from '@playwright/test';

import { installMockShellSession } from '../fixtures/mockSession';

const TRACE_ID = 'trace-1';
const LONG_BODY = JSON.stringify({
  mem_limit_pages: 256,
  mem_usage_pages: 256,
  trigger_comm: 'python3',
  payload: 'x'.repeat(2_048),
});

test.beforeEach(async ({ page }) => {
  await installMockShellSession(page);
  await page.route('**/api/v1/streams**', (route) =>
    route.fulfill({
      json: [{
        id: 'application-logs',
        name: 'application_logs',
        stream_type: 'logs',
        schema: {
          fields: [
            { name: '_timestamp', data_type: 'timestamp', nullable: false, indexed: true },
            { name: 'level', data_type: 'utf8', nullable: true, indexed: true },
            { name: 'service', data_type: 'utf8', nullable: true, indexed: true },
            { name: 'message', data_type: 'utf8', nullable: true, indexed: false },
            { name: 'body', data_type: 'utf8', nullable: true, indexed: false },
            { name: 'trace_id', data_type: 'utf8', nullable: true, indexed: true },
          ],
        },
        settings: { queryable: true },
      }],
    }),
  );
  await page.route('**/api/v1/web/logs', (route) =>
    route.fulfill({
      json: {
        items: [
          {
            _timestamp: 1_786_704_600_000_000,
            level: 'ERROR',
            service: 'checkout-api',
            message: 'checkout request failed',
            trace_id: TRACE_ID,
          },
          {
            _timestamp: 1_786_704_601_000_000,
            level: 'ERROR',
            service: 'node-agent',
            body: LONG_BODY,
          },
        ],
        next_cursor: null,
        previous_cursor: null,
        has_more: false,
      },
    }),
  );
  await page.route('**/api/v1/field_masking/effective/**', (route) =>
    route.fulfill({ json: { fields: [] } }),
  );
  await page.route(`**/api/v1/web/trace/${TRACE_ID}`, (route) =>
    route.fulfill({
      json: {
        trace_id: TRACE_ID,
        root_span_id: 'span-root',
        spans: [],
        truncated: false,
      },
    }),
  );
});

test('Logs overview pivots to the exact trace with investigation time', async ({
  page,
}) => {
  await page.goto('/logs');
  await expect(page.getByText('checkout request failed')).toBeVisible();

  await page.locator('[data-log-result-row="logs"]').first().click();
  const drawer = page.getByRole('complementary', {
    name: 'Log detail drawer',
  });
  await expect(drawer.getByText('Related signals')).toBeVisible();

  await drawer.locator(`[data-signal-type="trace_id"]`).first().click();
  const exactTrace = page.getByRole('link', { name: /Open current trace/ });
  await expect(exactTrace).toBeVisible();
  await exactTrace.click();

  await expect(page).toHaveURL(new RegExp(`/traces/${TRACE_ID}\\?`));
  const target = new URL(page.url());
  expect(target.searchParams.get('trace_id')).toBe(TRACE_ID);
  expect(target.searchParams.get('from')).toBeTruthy();
  expect(target.searchParams.get('to')).toBeTruthy();
  expect(target.searchParams.get('time')).toContain('..');
});

test('Log detail contains long messages without horizontal overflow', async ({
  page,
}) => {
  await page.goto('/logs');
  const rows = page.locator('[data-log-result-row="logs"]');
  await expect(rows).toHaveCount(2);
  await rows.nth(1).click();

  const drawer = page.getByRole('complementary', {
    name: 'Log detail drawer',
  });
  const scroller = drawer.locator('[data-log-detail-scroll]');
  const message = drawer.locator('[data-log-detail-message]');
  await expect(message).toContainText('mem_limit_pages');
  await expect.poll(async () => scroller.evaluate((element) => (
    element.scrollWidth <= element.clientWidth
  ))).toBe(true);
  await expect.poll(async () => message.evaluate((element) => (
    getComputedStyle(element).overflowWrap
  ))).toBe('anywhere');
});
