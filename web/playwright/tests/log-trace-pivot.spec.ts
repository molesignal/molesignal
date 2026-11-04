import { expect, test } from '@playwright/test';

import { explorerRead } from '../fixtures/explorerReads';
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
        streamType: 'logs',
        schema: {
          fields: [
            { name: '_timestamp', dataType: 'timestamp', nullable: false, indexed: true },
            { name: 'level', dataType: 'utf8', nullable: true, indexed: true },
            { name: 'service', dataType: 'utf8', nullable: true, indexed: true },
            { name: 'message', dataType: 'utf8', nullable: true, indexed: false },
            { name: 'body', dataType: 'utf8', nullable: true, indexed: false },
            { name: 'trace_id', dataType: 'utf8', nullable: true, indexed: true },
          ],
        },
        settings: { queryable: true },
      }],
    }),
  );
  await page.route(explorerRead('web/logs'), (route) =>
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
        nextCursor: null,
        previousCursor: null,
        hasMore: false,
      },
    }),
  );
  await page.route('**/api/v1/field_masking/effective/**', (route) =>
    route.fulfill({ json: { fields: [] } }),
  );
  await page.route(`**/api/v1/web/trace/${TRACE_ID}`, (route) =>
    route.fulfill({
      json: {
        traceId: TRACE_ID,
        rootSpanId: 'span-root',
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
  // The trace the log belongs to is a reference among the attributes at the head of the log.
  await expect(drawer.locator('[data-log-detail-attribute="trace_id"]')).toBeVisible();

  await drawer.locator(`[data-signal-type="trace_id"]`).first().click();
  const exactTrace = page.getByRole('link', { name: /Open current trace/ });
  await expect(exactTrace).toBeVisible();
  await exactTrace.click();

  await expect(page).toHaveURL(new RegExp(`/traces/${TRACE_ID}\\?`));
  const target = new URL(page.url());
  expect(target.searchParams.get('traceId')).toBe(TRACE_ID);
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
