/**
 * OR in the field query of Logs and Traces: alternatives typed in Code reach
 * the reads as a condition tree beside the filters that all hold, and the
 * Builder shows them as rows of filters, one row for each alternative.
 */
import type { Page } from '@playwright/test';

import { explorerRead, explorerReadBody } from '../fixtures/explorerReads';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { serveExplorerResults } from '../fixtures/mockExplorers';
import { serveTraceRed } from '../fixtures/mockTraceRed';

/** Room for a page to load and list: a load from the dev server takes several seconds while a suite runs. */
const LOADED = { timeout: 15_000 };

const LOG_STREAM = {
  id: 'application-logs',
  name: 'application_logs',
  label: 'application_logs',
  type: 'logs',
  streamType: 'logs',
  schema: {
    fields: [
      { name: '_timestamp', dataType: 'timestamp', nullable: false, indexed: true },
      { name: 'level', dataType: 'utf8', nullable: true, indexed: true },
      { name: 'status', dataType: 'int64', nullable: true, indexed: false },
      { name: 'message', dataType: 'utf8', nullable: true, indexed: false },
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

interface LogRead {
  filters?: Array<{ field: string; op: string; value: string }>;
  freeText?: string[];
  where?: unknown;
}

/** Serves one log stream and records what each list read asked for. */
async function serveLogs(page: Page): Promise<LogRead[]> {
  const reads: LogRead[] = [];
  await page.route('**/api/v1/streams**', (route) => route.fulfill({ json: [LOG_STREAM] }));
  await page.route(explorerRead('web/logs'), (route) => {
    reads.push(explorerReadBody<LogRead>(route.request()));
    return route.fulfill({
      json: {
        items: [
          { _timestamp: 1_785_628_332_000_000, level: 'ERROR', status: 500, message: 'upstream timeout' },
          { _timestamp: 1_785_628_333_000_000, level: 'INFO', status: 200, message: 'connection reset' },
        ],
        nextCursor: null,
        previousCursor: null,
        hasMore: false,
      },
    });
  });
  await page.route('**/api/v1/field_masking/effective/**', (route) => route.fulfill({ json: { fields: [] } }));
  return reads;
}

function front(page: Page) {
  return page.locator('[data-query-tab-panel][data-active="true"]');
}

function modes(page: Page) {
  return front(page).getByRole('group', { name: 'Query editor mode' });
}

/** The filters of each row of the Builder, as their chips read. */
async function rows(page: Page): Promise<string[][]> {
  return front(page)
    .locator('[data-query-builder-group]')
    .evaluateAll((groups) =>
      groups.map((group) =>
        Array.from(group.querySelectorAll('[data-query-filter], [data-query-filter-unsupported]')).map(
          (chip) => chip.textContent?.replace(/\s+/g, ' ').trim() ?? '',
        ),
      ),
    );
}

test.describe('OR in a field query', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('Logs asks alternatives as a condition tree beside the filters that all hold', async ({ page }) => {
    const reads = await serveLogs(page);
    const statement = `level = 'ERROR' AND (status >= 500 OR "connection reset")`;
    await page.goto(`/logs?q=${encodeURIComponent(statement)}`);

    await expect.poll(() => reads.length, LOADED).toBeGreaterThan(0);
    expect(reads.at(-1)?.filters).toMatchObject([{ field: 'level', op: '=', value: 'ERROR' }]);
    expect(reads.at(-1)?.freeText).toEqual([]);
    expect(reads.at(-1)?.where).toEqual({
      or: [{ field: 'status', op: '>=', value: '500', quoted: false }, { text: 'connection reset' }],
    });
  });

  test('Logs shows alternatives as rows of filters, and a row added there is one more OR', async ({ page }) => {
    const reads = await serveLogs(page);
    await page.goto(`/logs?q=${encodeURIComponent("level = 'ERROR' OR status >= 500")}`);
    await expect(front(page).getByText('upstream timeout').first()).toBeVisible(LOADED);

    await modes(page).getByRole('button', { name: 'Builder', exact: true }).click();
    const builder = front(page).locator('[data-query-builder]');
    await expect.poll(() => rows(page)).toEqual([['level=ERROR'], ['status>=500']]);
    await expect(builder.locator('[data-query-builder-or]')).toHaveText(['OR']);

    // Another row: field, comparison, value.
    await builder.getByRole('button', { name: 'Add an OR group' }).click();
    await page.getByPlaceholder('Search fields…').fill('level');
    await page.getByRole('option', { name: /level/ }).click();
    await page.getByRole('textbox', { name: 'Value' }).fill('WARN');
    await page.getByRole('button', { name: 'Add filter', exact: true }).click();
    await expect.poll(() => rows(page)).toEqual([['level=ERROR'], ['status>=500'], ['level=WARN']]);

    // Enter in a row's text box runs the query the rows describe.
    const before = reads.length;
    await builder.getByRole('textbox', { name: 'Search text of group 3', exact: true }).press('Enter');
    await expect.poll(() => reads.length).toBeGreaterThan(before);
    expect(reads.at(-1)?.where).toEqual({
      or: [
        { field: 'level', op: '=', value: 'ERROR', quoted: true },
        { field: 'status', op: '>=', value: '500', quoted: false },
        { field: 'level', op: '=', value: 'WARN', quoted: true },
      ],
    });

    await modes(page).getByRole('button', { name: 'Code', exact: true }).click();
    await expect(front(page).locator('.monaco-editor .view-lines').first()).toHaveText(
      "level = 'ERROR' OR status >= 500 OR level = 'WARN'",
    );
  });

  test('Traces asks the list and its histogram the alternatives, and says them under the editor', async ({ page }) => {
    await serveExplorerResults(page);
    const listed: URLSearchParams[] = [];
    const counted: URLSearchParams[] = [];
    page.on('request', (request) => {
      const url = new URL(request.url());
      if (url.pathname.endsWith('/api/v1/web/traces')) listed.push(url.searchParams);
      if (url.pathname.endsWith('/api/v1/web/traces/histogram')) counted.push(url.searchParams);
    });
    const statement = "status_code = 'ERROR' AND (service.name = 'checkout' OR span_count > 5)";
    await page.goto(`/traces?q=${encodeURIComponent(statement)}`);

    const where = {
      or: [
        { field: 'service.name', op: '=', value: 'checkout' },
        { field: 'span_count', op: '>', value: '5' },
      ],
    };
    await expect.poll(() => listed.length, LOADED).toBeGreaterThan(0);
    expect(JSON.parse(listed.at(-1)!.get('where')!)).toEqual(where);
    expect(JSON.parse(listed.at(-1)!.get('filters')!)).toEqual([{ field: 'status_code', op: '=', value: 'ERROR' }]);
    await expect.poll(() => counted.length, LOADED).toBeGreaterThan(0);
    expect(JSON.parse(counted.at(-1)!.get('where')!)).toEqual(where);

    // The editor keeps its fold with the tab: open it when it is folded.
    const alternatives = front(page).locator('[data-trace-query-alternatives]');
    const expand = front(page).getByRole('button', { name: 'Expand query' });
    await expect(alternatives.or(expand).first()).toBeVisible();
    if (await expand.isVisible()) await expand.click();
    await expect(alternatives).toHaveText(
      "(service.name = checkout OR span_count > 5)",
    );

    // In the Builder: one row, the alternatives in brackets kept as written.
    await modes(page).getByRole('button', { name: 'Builder', exact: true }).click();
    await expect.poll(() => rows(page)).toEqual([
      ['status_code=ERROR', "(service.name = 'checkout' OR span_count > 5)"],
    ]);
  });

  test('Traces RED counts the requests of either alternative', async ({ page }) => {
    await serveExplorerResults(page);
    const asked = await serveTraceRed(page);
    const statement = 'service.name = "catalog" OR service.name = "web"';
    await page.goto(`/traces?tab=red&q=${encodeURIComponent(statement)}`);

    await expect(page.locator('[data-red-group="catalog"]')).toBeVisible(LOADED);
    await expect(page.locator('[data-red-group="web"]')).toBeVisible();
    await expect(page.locator('[data-red-group="checkout-api"]')).toHaveCount(0);
    await expect(page.locator('[data-red-group="inventory"]')).toHaveCount(0);
    expect(JSON.parse(asked.breakdown.at(-1)!.get('where')!)).toEqual({
      or: [
        { field: 'service.name', op: '=', value: 'catalog' },
        { field: 'service.name', op: '=', value: 'web' },
      ],
    });
    // The alternatives are one step of the drill path.
    await expect(page.locator('[data-red-step]')).toHaveCount(1);
  });
});
