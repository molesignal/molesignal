import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';

/** The viewport the report renderer loads a page in. */
const RENDERER_VIEWPORT = { width: 1280, height: 800 };

const NOW_MICROS = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DAY_MICROS = 86_400_000_000;

/** Enough rows to run past one screen, one of them too long for any column. */
const ROW_COUNT = 60;
const UNBROKEN = `trace-${'9f3c'.repeat(120)}`;

interface QueryBody {
  language?: string;
  statement?: string;
  time_range?: { start: number; end: number };
  stream?: { name: string; stream_type: string };
  limit?: number;
}

/**
 * Answers the view's statement with rows of its own, and keeps what was asked:
 * the mock backend answers every SQL statement with the same single row.
 */
async function answerQueries(page: Page): Promise<QueryBody[]> {
  const asked: QueryBody[] = [];

  await page.route('**/api/v1/query', async (route) => {
    asked.push(route.request().postDataJSON() as QueryBody);
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        columns: ['_timestamp', 'service', 'level', 'message'],
        rows: Array.from({ length: ROW_COUNT }, (_, index) => [
          NOW_MICROS - index * 60_000_000,
          `service-${index + 1}`,
          'ERROR',
          index === 1 ? UNBROKEN : `upstream timed out after ${index + 1}s`,
        ]),
        scanned_rows: 48_210,
        took_ms: 31,
      }),
    });
  });

  return asked;
}

test.describe('saved view report', () => {
  test.use({ viewport: RENDERER_VIEWPORT });

  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('prints the rows the view returns over the report range, alone on the document', async ({ page }) => {
    const asked = await answerQueries(page);

    await page.goto('/saved-views?view=sv1&report_render=1&time=now-7d..now&report_tz=Asia%2FShanghai');

    // The renderer waits on this, and prints when it says ready.
    await expect(page.locator('html')).toHaveAttribute('data-report-render-state', 'ready', {
      timeout: 15_000,
    });

    // What the report is and what it covers, the statement, then the rows.
    await expect(page.getByRole('heading', { level: 1, name: 'Yesterday errors' })).toBeVisible();
    const facts = page.locator('[data-report-document-body] > header dl');
    await expect(facts).toContainText('Time range');
    await expect(facts).toContainText('2026-05-16 18:00 – 2026-05-23 18:00');
    await expect(facts).toContainText('Asia/Shanghai · UTC+8');
    await expect(facts).toContainText('app_logs');
    await expect(facts).toContainText(String(ROW_COUNT));
    await expect(page.getByRole('figure')).toContainText(`SELECT * FROM "app_logs" WHERE level = 'ERROR'`);

    const table = page.getByRole('table', { name: 'Results' });
    await expect(table.getByRole('columnheader')).toHaveText(['_timestamp', 'service', 'level', 'message']);
    await expect(table.locator('tbody tr')).toHaveCount(ROW_COUNT);
    // An instant is read as a time in the report's zone, not as the number the engine returns.
    await expect(table.locator('tbody tr').first().locator('td')).toHaveText([
      '2026-05-23 18:00:00',
      'service-1',
      'ERROR',
      'upstream timed out after 1s',
    ]);
    await expect(table.locator('tbody tr').last()).toContainText(`service-${ROW_COUNT}`);

    // The statement ran once, as the view states it, over the range the report states.
    expect(asked).toHaveLength(1);
    expect(asked[0]).toMatchObject({
      language: 'sql',
      statement: `SELECT * FROM "app_logs" WHERE level = 'ERROR'`,
      time_range: { start: NOW_MICROS - 7 * DAY_MICROS, end: NOW_MICROS },
      stream: { name: 'app_logs', stream_type: 'logs' },
    });

    // None of the application is on the page: no navigation, no top bar, no list to manage.
    await expect(page.getByTestId('sidebar-toggle')).toHaveCount(0);
    await expect(page.locator('nav')).toHaveCount(0);
    await expect(page.locator('main#main')).toHaveCount(0);
    await expect(page.getByRole('button')).toHaveCount(0);
    await expect(page.getByRole('link')).toHaveCount(0);
    await expect(page.locator('select, input, textarea')).toHaveCount(0);

    // A report is light whatever the reader's theme is (the suite runs dark).
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');

    // The document is as long as the rows and no wider than the page: the last row is in it,
    // and one PDF page as tall as the content holds it.
    const content = await page.evaluate(() => {
      // How many lines a cell's text is set on.
      const lines = (cell: Element | null | undefined) => {
        if (!cell) return 0;
        const range = document.createRange();
        range.selectNodeContents(cell);
        return new Set(Array.from(range.getClientRects(), (rect) => Math.round(rect.top))).size;
      };
      const headers = Array.from(document.querySelectorAll('thead th'));
      const secondRow = document.querySelectorAll('tbody tr')[1];

      return {
        // Measured the way the renderer measures it: the root element's own box.
        height: document.documentElement.getBoundingClientRect().height,
        window: window.innerHeight,
        sideways: document.documentElement.scrollWidth - document.documentElement.clientWidth,
        tableRight: document.querySelector('table')?.getBoundingClientRect().right ?? 0,
        lastRowBottom:
          (document.querySelector('tbody tr:last-child')?.getBoundingClientRect().bottom ?? 0) +
          window.scrollY,
        headerLines: headers.map(lines),
        shortCellLines: Array.from(secondRow?.querySelectorAll('td') ?? [])
          .slice(0, 3)
          .map(lines),
        longCellLines: lines(secondRow?.querySelector('td:last-child')),
      };
    });
    expect(content.height).toBeGreaterThan(content.window);
    expect(content.lastRowBottom).toBeLessThanOrEqual(content.height);
    expect(content.sideways).toBe(0);
    expect(content.tableRight).toBeLessThanOrEqual(RENDERER_VIEWPORT.width);
    // A short value keeps its words whole; the value too long for its column wraps inside it.
    expect(content.headerLines).toEqual([1, 1, 1, 1]);
    expect(content.shortCellLines).toEqual([1, 1, 1]);
    expect(content.longCellLines).toBeGreaterThan(1);

    // Sized the way the renderer sizes it: the content rounded up, and a pixel to spare.
    const pdf = await page.pdf({
      width: `${RENDERER_VIEWPORT.width}px`,
      height: `${Math.ceil(content.height) + 1}px`,
      printBackground: true,
      margin: { top: '0', right: '0', bottom: '0', left: '0' },
    });
    const pages = pdf.toString('latin1').match(/\/Type\s*\/Page[^s]/g) ?? [];
    expect(pages).toHaveLength(1);
  });

  test("covers the view's own look-back when the report states no range", async ({ page }) => {
    const asked = await answerQueries(page);

    await page.goto('/saved-views?view=sv1&report_render=1');

    await expect(page.locator('html')).toHaveAttribute('data-report-render-state', 'ready', {
      timeout: 15_000,
    });
    // The mock view looks back 24 hours.
    expect(asked[0]?.time_range).toEqual({ start: NOW_MICROS - DAY_MICROS, end: NOW_MICROS });
  });

  test('is still the list a person manages without the report parameter', async ({ page }) => {
    await page.goto('/saved-views?view=sv1');

    await expect(page.getByTestId('sidebar-toggle')).toBeVisible();
    await expect(page.getByRole('button', { name: 'New saved view' })).toBeVisible();
    await expect(page.getByText('Yesterday errors')).toBeVisible();
    await expect(page.locator('html')).not.toHaveAttribute('data-report-document', '');
    await expect(page.locator('html')).not.toHaveAttribute('data-report-render-state', /.*/);
  });

  test('fails the report with the reason instead of printing an error page', async ({ page }) => {
    await page.goto('/saved-views?view=does-not-exist&report_render=1');

    await expect(page.locator('html')).toHaveAttribute('data-report-render-state', 'error', {
      timeout: 15_000,
    });
    await expect(page.locator('html')).toHaveAttribute(
      'data-report-render-error',
      'saved view does-not-exist not found',
    );
    await expect(page.getByRole('alert')).toHaveText('saved view does-not-exist not found');
    await expect(page.getByTestId('sidebar-toggle')).toHaveCount(0);
    await expect(page.getByRole('table')).toHaveCount(0);
  });

  test('fails the report when the statement cannot be run', async ({ page }) => {
    await page.route('**/api/v1/query', (route) =>
      route.fulfill({
        status: 400,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'query could not be planned: check the SQL syntax' }),
      }),
    );

    await page.goto('/saved-views?view=sv1&report_render=1&time=now-24h..now');

    await expect(page.locator('html')).toHaveAttribute('data-report-render-state', 'error', {
      timeout: 15_000,
    });
    await expect(page.getByRole('alert')).toHaveText('query could not be planned: check the SQL syntax');
    await expect(page.getByRole('heading', { level: 1 })).toHaveCount(0);
  });
});
