import type { Locator, Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const field = (name: string, dataType = 'utf8') => ({ name, data_type: dataType, nullable: true, indexed: false });
const stream = (name: string, type: 'logs' | 'traces', fields: ReturnType<typeof field>[]) => ({
  id: `${type}-stream`,
  name,
  stream_type: type,
  schema: { fields },
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
});

/** Opens the share panel of the explorer on the page. */
async function openShare(page: Page): Promise<Locator> {
  await page.locator('[data-query-share]').click();
  const panel = page.getByRole('dialog', { name: 'Share this query' });
  await expect(panel).toBeVisible();

  return panel;
}

const linkOf = async (panel: Locator) =>
  new URL(await panel.getByRole('textbox', { name: 'Link to this query' }).inputValue());

/** The two ends of the `time` a link carries, in milliseconds. */
const momentsOf = (link: URL) => (link.searchParams.get('time') ?? '').split('..').map((moment) => Date.parse(moment));

test.describe('Sharing an explorer query', () => {
  test.beforeEach(async ({ page, mockServer, context }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mountMockRoutes(page, mockServer.port);
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  });

  test('Logs: the link carries the stream, the query and the moments on screen, and opens them again', async ({
    page,
  }) => {
    const now = Date.now();
    const asked: { from?: number; to?: number; filters?: { field: string; value: string }[] }[] = [];
    await page.route('**/api/v1/streams**', (route) =>
      route.fulfill({
        json: [
          stream('application_logs', 'logs', [
            { name: '_timestamp', data_type: 'timestamp', nullable: false, indexed: true },
            field('level'),
            field('message'),
          ]),
        ],
      }),
    );
    await page.route('**/api/v1/web/logs', (route) => {
      asked.push(route.request().postDataJSON());

      return route.fulfill({
        json: {
          items: [{ _timestamp: (now - 30_000) * 1_000, level: 'ERROR', message: 'failed to reserve stock' }],
          next_cursor: null,
          previous_cursor: null,
          has_more: false,
        },
      });
    });
    await page.route('**/api/v1/field_masking/effective/**', (route) => route.fulfill({ json: { fields: [] } }));

    await page.goto(`/logs?stream=application_logs&q=${encodeURIComponent("level = 'ERROR'")}`);
    await expect(page.locator('[data-log-result-row="logs"]')).toHaveCount(1);

    const panel = await openShare(page);
    let link = await linkOf(panel);
    expect(link.pathname).toBe('/logs');
    expect(link.searchParams.get('stream')).toBe('application_logs');
    expect(link.searchParams.get('q')).toBe("level = 'ERROR'");
    // "Last 1 hour" is fixed to the hour that ended when the panel was opened.
    await expect(panel.getByRole('radio', { name: /^Current time selection/ })).toBeChecked();
    const [from, to] = momentsOf(link);
    expect(to! - from!).toBe(3_600_000);
    // By the page's own clock, which the fixture sets.
    expect(Math.abs((await page.evaluate(() => Date.now())) - to!)).toBeLessThan(30_000);

    // Kept moving, the window every page opens on needs no saying.
    await panel.getByRole('radio', { name: 'Last 1 hour, from when it is opened' }).click();
    link = await linkOf(panel);
    expect(link.searchParams.has('time')).toBe(false);
    expect(link.searchParams.get('q')).toBe("level = 'ERROR'");

    // The copy button copies what the field shows, and says that it has.
    await panel.getByRole('radio', { name: /^Current time selection/ }).click();
    await panel.getByRole('button', { name: 'Copy link' }).click();
    await expect(panel.getByRole('button', { name: 'Link copied' })).toBeVisible();
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    expect(copied).toBe((await linkOf(panel)).toString());

    // Opened, the link is the same query over the same moments, not the last hour of whoever opens it.
    await page.keyboard.press('Escape');
    asked.length = 0;
    await page.goto(copied);
    await expect(page.locator('[data-log-result-row="logs"]')).toHaveCount(1);
    await expect(page.locator('[data-query-workbench-appearance="surface"]')).toContainText("level = 'ERROR'");
    const [fixedFrom, fixedTo] = momentsOf(new URL(copied));
    const request = asked.at(-1)!;
    expect(request.from).toBe(fixedFrom! * 1_000);
    expect(request.to).toBe(fixedTo! * 1_000);
    expect(request.filters).toEqual([expect.objectContaining({ field: 'level', value: 'ERROR' })]);
  });

  test('Traces: the link carries the view and the query that is applied', async ({ page }) => {
    const now = Date.now();
    await page.route('**/api/v1/streams**', (route) =>
      route.fulfill({
        json: [stream('default', 'traces', [field('trace_id'), field('span_id'), field('name'), field('service.name')])],
      }),
    );
    await page.route('**/api/v1/web/topology**', (route) => route.fulfill({ json: { nodes: [], edges: [] } }));
    await page.route(/\/api\/v1\/web\/traces(\?.*)?$/, (route) =>
      route.fulfill({
        json: {
          items: [
            {
              trace_id: '7c1f0e03a94b4d2e8f3a5b6c7d8e9f03',
              operation: 'POST /api/v1/checkout',
              service: 'gateway',
              start_ns: (now - 30_000) * 1_000_000,
              duration_ms: 184,
              span_count: 7,
              error_count: 0,
            },
          ],
          next_cursor: null,
          previous_cursor: null,
          has_more: false,
        },
      }),
    );

    await page.goto(`/traces?tab=traces&sort=duration_desc&q=${encodeURIComponent("service_name = 'gateway'")}`);
    await expect(page.locator('[data-workspace-pane="trace-results"] button[aria-pressed]')).toHaveCount(1);

    const link = await linkOf(await openShare(page));
    expect(link.pathname).toBe('/traces');
    expect(link.searchParams.get('tab')).toBe('traces');
    expect(link.searchParams.get('sort')).toBe('duration_desc');
    expect(link.searchParams.get('q')).toBe("service_name = 'gateway'");
    // The moments are those of the range the page is on, whatever it is: a difference that is a time.
    const [from, to] = momentsOf(link);
    expect(to! - from!).toBeGreaterThan(0);
    expect(Math.abs((await page.evaluate(() => Date.now())) - to!)).toBeLessThan(30_000);
  });

  test('Metrics: the link carries the query and how it is drawn', async ({ page }) => {
    await page.goto(`/metrics?promql=${encodeURIComponent('sum(rate(http_requests_total[5m]))')}&draw=bar`);
    await expect(page.locator('[data-query-share]')).toBeVisible();

    const link = await linkOf(await openShare(page));
    expect(link.pathname).toBe('/metrics');
    expect(link.searchParams.get('promql')).toBe('sum(rate(http_requests_total[5m]))');
    expect(link.searchParams.get('draw')).toBe('bar');
    const [from, to] = momentsOf(link);
    expect(to! - from!).toBe(3_600_000);
  });
});
