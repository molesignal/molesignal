import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

/** The viewport the report renderer loads a page in. */
const RENDERER_VIEWPORT = { width: 1280, height: 800 };

/** Fills the mock dashboard with enough panels to run past one screen. */
async function seedTallDashboard(page: Page): Promise<void> {
  await page.goto('/signin');
  const stored = (await page.evaluate(() =>
    fetch('/api/v1/dashboards/d1').then((response) => response.json()),
  )) as { model: Record<string, unknown> };

  const panels = Array.from({ length: 8 }, (_, index) => ({
    kind: 'panel',
    id: `panel-${index}`,
    title: `Service ${index + 1} request rate`,
    description: 'Five-minute request rate.',
    gridPos: { x: (index % 2) * 12, y: Math.floor(index / 2) * 14, w: 12, h: 14, minW: 2, minH: 4 },
    queryOptions: {},
    queries: [
      {
        refId: 'A',
        enabled: true,
        dataSourceType: 'metrics',
        dataSourceId: 'source-1',
        query: { language: 'promql', expression: `rate(http_requests_total{job="svc-${index}"}[5m])` },
      },
    ],
    transformations: [],
    visualization: { type: 'time_series', schemaVersion: 1, options: {} },
    fieldConfig: {},
    overrides: [],
    links: [],
  }));
  const model = {
    ...stored.model,
    description: 'How the checkout path is doing, service by service.',
    variables: [
      {
        id: 'variable-env',
        name: 'env',
        label: 'Environment',
        type: 'custom',
        options: [{ label: 'production', value: 'production', selected: true }],
        multi: false,
        includeAll: false,
        hide: 'none',
        refresh: 'never',
      },
    ],
    elements: panels,
  };

  const status = await page.evaluate(
    (next) =>
      fetch('/api/v1/dashboards/d1', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ model: next }),
      }).then((response) => response.status),
    model,
  );
  expect(status).toBe(200);
}

test.describe('dashboard report', () => {
  test.use({ viewport: RENDERER_VIEWPORT });

  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await seedTallDashboard(page);
  });

  test('prints the dashboard alone, on a document as long as the dashboard', async ({ page }) => {
    await page.goto('/dashboards/d1?report_render=1&time=now-7d..now&report_tz=Asia%2FShanghai');

    // The renderer waits on this, and prints when it says ready.
    await expect(page.locator('html')).toHaveAttribute('data-report-render-state', 'ready', {
      timeout: 15_000,
    });

    // What the report is and what it covers, then the panels.
    await expect(page.getByRole('heading', { level: 1, name: 'Web overview' })).toBeVisible();
    await expect(page.getByText('How the checkout path is doing, service by service.')).toBeVisible();
    const facts = page.locator('[data-report-document-body] > header dl');
    await expect(facts).toContainText('Time range');
    await expect(facts).toContainText('Asia/Shanghai · UTC+8');
    await expect(facts).toContainText('Environment');
    await expect(facts).toContainText('production');
    await expect(page.locator('[data-printed-panel]')).toHaveCount(8);
    await expect(page.getByRole('heading', { level: 3, name: 'Service 8 request rate' })).toBeVisible();

    // None of the application is on the page: no navigation, no top bar, no page toolbar.
    await expect(page.getByTestId('sidebar-toggle')).toHaveCount(0);
    await expect(page.locator('nav')).toHaveCount(0);
    await expect(page.locator('main#main')).toHaveCount(0);
    for (const name of ['Edit', 'Share', 'Set fullscreen', 'Refresh all panels', 'More actions']) {
      await expect(page.getByRole('button', { name })).toHaveCount(0);
    }
    await expect(page.getByRole('button', { name: /Open panel menu/ })).toHaveCount(0);
    await expect(page.locator('select')).toHaveCount(0);

    // A report is light whatever the reader's theme is (the suite runs dark).
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');

    // The document is the dashboard's length, not the window's: the last panel is in it, and
    // one PDF page as tall as the content holds all of it.
    const content = await page.evaluate(() => ({
      // Measured the way the renderer measures it: the root element's own box.
      height: document.documentElement.getBoundingClientRect().height,
      window: window.innerHeight,
      sideways: document.documentElement.scrollWidth - document.documentElement.clientWidth,
      lastPanelBottom: Array.from(document.querySelectorAll('[data-printed-panel]'))
        .map((panel) => panel.getBoundingClientRect().bottom + window.scrollY)
        .reduce((lowest, bottom) => Math.max(lowest, bottom), 0),
    }));
    expect(content.height).toBeGreaterThan(content.window);
    expect(content.lastPanelBottom).toBeLessThanOrEqual(content.height);
    expect(content.sideways).toBe(0);

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

  test('is still the page a person works in without the report parameter', async ({ page }) => {
    await page.goto('/dashboards/d1');

    await expect(page.getByTestId('sidebar-toggle')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Edit' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Open panel menu: Service 1 request rate' })).toHaveCount(1);
    await expect(page.locator('html')).not.toHaveAttribute('data-report-document', '');
    await expect(page.locator('html')).not.toHaveAttribute('data-report-render-state', /.*/);
  });

  test('fails the report with the reason instead of printing an error page', async ({ page }) => {
    await page.goto('/dashboards/does-not-exist?report_render=1');

    await expect(page.locator('html')).toHaveAttribute('data-report-render-state', 'error', {
      timeout: 15_000,
    });
    const reason = await page.locator('html').getAttribute('data-report-render-error');
    expect(reason).toBeTruthy();
    await expect(page.getByRole('alert')).toHaveText(reason ?? '');
    await expect(page.getByTestId('sidebar-toggle')).toHaveCount(0);
  });
});
