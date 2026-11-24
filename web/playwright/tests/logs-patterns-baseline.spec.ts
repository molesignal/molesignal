import { expect, test, type Page } from '@playwright/test';

import { explorerRead } from '../fixtures/explorerReads';
import { installMockShellSession } from '../fixtures/mockSession';

const field = (name: string, dataType = 'utf8') => ({ name, dataType: dataType, nullable: true, indexType: 'none' });
const STREAM = {
  id: 'application-logs',
  name: 'application_logs',
  streamType: 'logs',
  schema: {
    fields: [
      { name: '_timestamp', dataType: 'timestamp', nullable: false, indexType: 'skip' },
      field('level'),
      field('service.name'),
      field('message'),
      field('trace_id'),
      field('http.method'),
      field('http.route'),
      field('http.status_code', 'int64'),
      field('duration_ms', 'float64'),
      field('k8s.pod.name'),
      field('host.name'),
      field('user.id'),
    ],
  },
  retention: { days: 7 },
  effectiveRetention: { days: 7 },
  settings: {
    description: null,
    indexRules: [],
    retentionFilter: null,
    keepConditions: [],
    maxQueryRangeHours: null,
    flattenLevel: null,
    useStreamStatsForPartitioning: false,
    storeOriginalData: false,
    enableDistinctValues: true,
    queryable: true,
  },
  createdAt: 1,
  updatedAt: 1,
};
const LEVELS = ['INFO', 'ERROR', 'WARN', 'INFO', 'DEBUG', 'INFO'];
const FAILURE = 'failed to reserve stock: sku=SKU-48213 requested=4 available=1';
const word = (text: string, wild: string | null = null) => ({ lead: '', trail: '', text, wild, unit: '' });

async function serveLogs(page: Page) {
  await installMockShellSession(page);
  // The newest rows of the last minute, 1.37 s apart, so no two share a millisecond.
  const now = Date.now();
  const items = LEVELS.map((level, index) => ({
    _timestamp: (now - 5_000 - index * 1_370) * 1_000,
    level,
    'service.name': index % 2 === 0 ? 'checkout-api' : 'payments',
    message: level === 'ERROR' ? FAILURE : `request ${index} completed`,
    trace_id: `trace-${index}`,
    'http.method': 'GET',
    'http.route': '/api/v1/cart/:id',
    'http.status_code': level === 'ERROR' ? 504 : 200,
    duration_ms: 20 + index,
    'k8s.pod.name': `checkout-api-7d9f8b6c5-p${index}`,
    'host.name': `ip-10-0-0-${index}.internal`,
    'user.id': `u_${index}`,
  }));
  await page.route('**/api/v1/streams**', (route) => route.fulfill({ json: [STREAM] }));
  await page.route(explorerRead('web/logs'), (route) =>
    route.fulfill({ json: { items, nextCursor: null, previousCursor: null, hasMore: false } }),
  );
  await page.route('**/api/v1/field_masking/effective/**', (route) =>
    route.fulfill({ json: { fields: [] } }),
  );
}

test('counts the patterns over the whole range on the server, and tells which are new or rising against a baseline', async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await serveLogs(page);
  const asked: Record<string, unknown>[] = [];
  await page.route('**/api/v1/web/logs/patterns', (route) => {
    const body = route.request().postDataJSON() as Record<string, unknown>;
    asked.push(body);
    return route.fulfill({
      json: {
        stream: 'application_logs',
        field: 'message',
        levelField: 'level',
        timeRange: { startTime: body.from, endTime: body.to },
        logs: 4_000,
        read: 600,
        unread: 0,
        sampled: true,
        baseline: { kind: body.baseline, startTime: 0, endTime: 1, logs: 1_000, read: 600, sampled: true },
        newPatterns: 1,
        surgingPatterns: 1,
        droppingPatterns: 0,
        patternCount: 2,
        patterns: [
          {
            pattern: 'request <number> completed',
            tokens: [word('request'), word('3', 'number'), word('completed')],
            count: 500,
            share: 0.83,
            estimatedCount: 3_333,
            firstSeen: 1,
            lastSeen: 2,
            levels: [{ level: 'INFO', count: 400 }, { level: 'WARN', count: 100 }],
            baselineCount: 150,
            change: 'surging',
            ratio: 3.2,
            fragment: 'completed',
            example: 'request 3 completed',
          },
          {
            pattern: 'upstream reset by peer',
            tokens: [word('upstream'), word('reset'), word('by'), word('peer')],
            count: 2,
            share: 0.01,
            estimatedCount: 13,
            firstSeen: 1,
            lastSeen: 2,
            levels: [{ level: 'ERROR', count: 2 }],
            baselineCount: 0,
            change: 'new',
            fragment: 'upstream reset by peer',
            example: 'upstream reset by peer',
          },
        ],
      },
    });
  });

  await page.goto('/logs');
  await expect(page.locator('[data-log-result-row="logs"]')).toHaveCount(LEVELS.length);
  await page.locator('[data-log-result-views]').getByRole('button', { name: 'Patterns' }).click();

  const patterns = page.locator('[data-log-pattern]');
  await expect(patterns).toHaveCount(2);
  await expect(patterns.first().locator('[data-log-pattern-change="surging"]')).toHaveText('×3.2');
  await expect(patterns.first()).toContainText('≈3333');
  await expect(patterns.nth(1).locator('[data-log-pattern-change="new"]')).toHaveText('New');
  await expect(patterns.nth(1)).toHaveAttribute('data-log-level', 'error');

  // What the counts are of: every event of the range, the patterns read from a sample of it.
  const basis = page.locator('[data-log-patterns-basis]');
  await expect(basis).toContainText('Counted over all 4,000 events in range');
  await expect(basis).toContainText('patterns read from 600 sampled across it');
  expect(asked[0]).toMatchObject({ stream: 'application_logs', baseline: 'previous' });

  // A pattern opens on the logs read that are of it; one none of them is says one of its messages.
  await patterns.first().locator('[data-log-pattern-text]').click();
  await expect(patterns.first().locator('[data-log-pattern-examples]').getByRole('button')).toHaveCount(5);
  await patterns.nth(1).locator('[data-log-pattern-text]').click();
  await expect(patterns.nth(1).locator('[data-log-pattern-example-only]')).toContainText('upstream reset by peer');

  // Another baseline asks again.
  await basis.getByRole('combobox', { name: 'Compare with' }).click();
  await page.getByRole('option', { name: 'Compared with a week before' }).click();
  await expect.poll(() => asked.at(-1)?.baseline).toBe('week_before');
});
