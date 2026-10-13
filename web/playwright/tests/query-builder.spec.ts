import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const LOG_STREAM = {
  id: 'application-logs',
  name: 'application_logs',
  label: 'application_logs',
  type: 'logs',
  stream_type: 'logs',
  schema: {
    fields: [
      { name: '_timestamp', data_type: 'timestamp', nullable: false, indexed: true },
      { name: 'level', data_type: 'utf8', nullable: true, indexed: true },
      { name: 'status', data_type: 'int64', nullable: true, indexed: false },
      { name: 'message', data_type: 'utf8', nullable: true, indexed: false },
    ],
  },
  retention: { days: 7 },
  effective_retention: { days: 7 },
  settings: {
    description: null,
    index_rules: [],
    retention_filter: null,
    keep_conditions: [],
    max_query_range_hours: null,
    flatten_level: null,
    use_stream_stats_for_partitioning: false,
    store_original_data: false,
    enable_distinct_values: true,
    queryable: true,
  },
  created_at_micros: 1,
  updated_at_micros: 1,
};

interface LogListRequest {
  filters?: Array<{ field: string; op: string; value: string }>;
  free_text?: string[];
}

/** Serves one log stream and records what each list request asked for. */
async function serveLogs(page: Page): Promise<LogListRequest[]> {
  const requests: LogListRequest[] = [];
  await page.route('**/api/v1/streams**', (route) => route.fulfill({ json: [LOG_STREAM] }));
  await page.route('**/api/v1/web/logs', (route) => {
    requests.push(route.request().postDataJSON() as LogListRequest);
    return route.fulfill({
      json: {
        items: [
          { _timestamp: 1_785_628_332_000_000, level: 'ERROR', status: 500, message: 'upstream timeout' },
          { _timestamp: 1_785_628_333_000_000, level: 'INFO', status: 200, message: 'order accepted' },
        ],
        next_cursor: null,
        previous_cursor: null,
        has_more: false,
      },
    });
  });
  await page.route('**/api/v1/field_masking/effective/**', (route) =>
    route.fulfill({ json: { fields: [] } }),
  );
  return requests;
}

function front(page: Page) {
  return page.locator('[data-query-tab-panel][data-active="true"]');
}

function modes(page: Page) {
  return front(page).getByRole('group', { name: 'Query editor mode' });
}

/**
 * Writes a statement into the empty Code editor. The text goes in as one input,
 * the way a paste does: typed key by key, the editor's own suggestions and
 * quote pairing would rewrite it on the way.
 */
async function writeStatement(page: Page, statement: string) {
  await front(page).getByLabel('Log field query editor').click({ force: true });
  await page.keyboard.insertText(statement);
}

function run(page: Page) {
  return front(page).locator('[data-query-toolbar]').getByRole('button', { name: 'Run', exact: true });
}

test.describe('Code and Builder', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('every explorer writes a query the same two ways, starting in Code', async ({ page }) => {
    await serveLogs(page);
    for (const path of ['/metrics', '/logs', '/traces']) {
      await page.goto(path);
      const buttons = modes(page).getByRole('button');
      await expect(buttons).toHaveText(['Code', 'Builder']);
      await expect(buttons.first()).toHaveAttribute('aria-pressed', 'true');
      // The old language switch is gone: the editor reads what is typed instead.
      await expect(front(page).getByRole('button', { name: 'SQL', exact: true })).toHaveCount(0);
      await expect(front(page).getByRole('button', { name: 'Fields', exact: true })).toHaveCount(0);
    }
  });

  test('Code takes a field query as it is written', async ({ page }) => {
    const requests = await serveLogs(page);
    await page.goto('/logs');
    await expect(front(page).getByText('upstream timeout').first()).toBeVisible();
    await front(page).getByRole('button', { name: 'Expand query' }).click();

    await writeStatement(page, "level = 'ERROR' AND status >= 500");
    // The editor is named "Query" whatever is typed; the language it read is said under it.
    await expect(front(page).locator('[data-query-editor-state="expanded"]')).toContainText('Fields');
    const before = requests.length;
    await run(page).click();

    await expect.poll(() => requests.length).toBeGreaterThan(before);
    expect(requests.at(-1)?.filters).toMatchObject([
      { field: 'level', op: '=', value: 'ERROR' },
      { field: 'status', op: '>=', value: '500' },
    ]);
  });

  test('Code takes SQL in the same editor', async ({ page }) => {
    await serveLogs(page);
    const statements: string[] = [];
    page.on('request', (request) => {
      if (request.method() !== 'POST' || new URL(request.url()).pathname !== '/api/v1/query') return;
      const body = request.postDataJSON() as { language?: string; statement?: string };
      if (body.statement) statements.push(body.statement);
    });
    await page.goto('/logs');
    await front(page).getByRole('button', { name: 'Expand query' }).click();

    const sql = 'SELECT level, count(*) FROM "application_logs" GROUP BY level';
    await writeStatement(page, sql);
    // What was typed opens with SELECT, so the editor is on SQL without being told.
    await expect(front(page).locator('[data-query-editor-state="expanded"]')).toContainText('SQL');
    await run(page).click();

    await expect.poll(() => statements.at(-1)).toBe(sql);
  });

  test('Builder shows the same query as filters and writes them back', async ({ page }) => {
    const requests = await serveLogs(page);
    await page.goto(`/logs?q=${encodeURIComponent("level = 'ERROR'")}`);
    await expect(front(page).getByText('upstream timeout').first()).toBeVisible();

    await modes(page).getByRole('button', { name: 'Builder', exact: true }).click();
    const builder = front(page).locator('[data-query-builder]');
    await expect(builder.locator('[data-query-filter]')).toHaveText(['level=ERROR']);

    // Add `status >= 500`: field, comparison, value.
    await builder.getByRole('button', { name: 'Add a filter' }).click();
    await page.getByPlaceholder('Search fields…').fill('status');
    await page.getByRole('option', { name: /status/ }).click();
    await page.getByRole('group', { name: 'Comparison' }).getByRole('button', { name: '>=' }).click();
    await page.getByRole('textbox', { name: 'Value' }).fill('500');
    await page.getByRole('button', { name: 'Add filter', exact: true }).click();
    await expect(builder.locator('[data-query-filter]')).toHaveText(['level=ERROR', 'status>=500']);

    // Enter in the text box runs the query the filters describe.
    const before = requests.length;
    const text = builder.getByRole('textbox', { name: 'Search text' });
    await text.fill('timeout');
    await text.press('Enter');
    await expect.poll(() => requests.length).toBeGreaterThan(before);
    expect(requests.at(-1)?.filters).toMatchObject([
      { field: 'level', op: '=', value: 'ERROR' },
      { field: 'status', op: '>=', value: '500' },
    ]);
    expect(requests.at(-1)?.free_text).toEqual(['timeout']);

    // Code holds the statement the filters wrote.
    await modes(page).getByRole('button', { name: 'Code', exact: true }).click();
    await expect(front(page).locator('.monaco-editor .view-lines').first()).toHaveText(
      "level = 'ERROR' AND status >= 500 AND timeout",
    );
  });

  test('a filter is removed from its chip', async ({ page }) => {
    await serveLogs(page);
    await page.goto(`/logs?q=${encodeURIComponent("level = 'ERROR' AND status >= 500")}`);
    await modes(page).getByRole('button', { name: 'Builder', exact: true }).click();
    const builder = front(page).locator('[data-query-builder]');

    await builder.getByRole('button', { name: 'Remove filter level = ERROR' }).click();

    await expect(builder.locator('[data-query-filter]')).toHaveText(['status>=500']);
  });

  test('Traces builds its filters the same way', async ({ page }) => {
    await page.goto(`/traces?q=${encodeURIComponent("service_name = 'checkout'")}`);
    await modes(page).getByRole('button', { name: 'Builder', exact: true }).click();

    const builder = front(page).locator('[data-query-builder]');
    // The alias in the link is shown as the field the stream has.
    await expect(builder.locator('[data-query-filter]')).toHaveText(['service.name=checkout']);
    await expect(builder.getByRole('button', { name: 'Add a filter' })).toBeVisible();
    await expect(builder.getByRole('textbox', { name: 'Search text' })).toBeVisible();
  });
});
