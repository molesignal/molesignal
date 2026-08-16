import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

type ExploreCase = {
  title: string;
  dataSourceType: 'metrics' | 'logs';
  query: Record<string, unknown>;
};

async function setDashboardPanel(page: Page, panel: ExploreCase): Promise<void> {
  await page.goto('/home');
  await page.evaluate(async (nextPanel) => {
    const response = await fetch('/api/v1/dashboards/d1');
    const dashboard = (await response.json()) as {
      model: Record<string, unknown>;
    };
    const model = {
      ...dashboard.model,
      refreshSettings: {
        enabled: false,
        mode: 'off',
        allowedIntervals: ['off'],
      },
      elements: [
        {
          kind: 'panel',
          id: 'panel-explore-continuity',
          title: nextPanel.title,
          gridPos: { x: 0, y: 0, w: 12, h: 18, minW: 2, minH: 4 },
          queryOptions: {},
          queries: [
            {
              refId: 'A',
              enabled: true,
              dataSourceType: nextPanel.dataSourceType,
              dataSourceId: 'source-1',
              query: nextPanel.query,
            },
          ],
          transformations: [],
          visualization: {
            type: 'time_series',
            schemaVersion: 1,
            options: {},
          },
          fieldConfig: {},
          overrides: [],
          links: [],
        },
      ],
    };
    const update = await fetch('/api/v1/dashboards/d1', {
      method: 'PUT',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ model }),
    });
    if (!update.ok) throw new Error(`Could not seed dashboard: ${update.status}`);
  }, panel);
}

async function openExplore(page: Page, title: string): Promise<void> {
  await page.goto('/dashboards/d1');
  const menu = page.getByRole('button', { name: `Open panel menu: ${title}` });
  await expect(menu).toBeVisible({ timeout: 10_000 });
  await menu.click();
  await page.getByRole('menuitem', { name: 'Explore data' }).click();
}

test.describe('dashboard → Explore continuity', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('PromQL panel opens Metrics with the executable query and context', async ({
    page,
  }) => {
    const expression = 'rate(http_requests_total{service="checkout"}[5m])';
    await setDashboardPanel(page, {
      title: 'Checkout request rate',
      dataSourceType: 'metrics',
      query: { language: 'promql', expression },
    });
    await openExplore(page, 'Checkout request rate');

    await expect(page).toHaveURL(/\/metrics\?/);
    const url = new URL(page.url());
    expect(url.searchParams.get('promql')).toBe(expression);
    expect(url.searchParams.get('time')).not.toBeNull();
    expect(url.searchParams.get('data_source')).toBe('source-1');
    await expect(page.locator('.monaco-editor .view-lines')).toContainText(expression);
  });

  test('SQL Logs panel opens the SQL editor without reinterpreting the statement', async ({
    page,
  }) => {
    const statement = 'SELECT * FROM "app_logs" WHERE service = \'checkout\'';
    await setDashboardPanel(page, {
      title: 'Checkout logs',
      dataSourceType: 'logs',
      query: {
        language: 'sql',
        statement,
        stream: 'app_logs',
        streamType: 'logs',
      },
    });
    await openExplore(page, 'Checkout logs');

    await expect(page).toHaveURL(/\/logs\?/);
    const url = new URL(page.url());
    expect(url.searchParams.get('sql')).toBe(statement);
    expect(url.searchParams.get('q')).toBeNull();
    expect(url.searchParams.get('stream')).toBe('app_logs');
    await expect(page.locator('.monaco-editor .view-lines')).toContainText(statement);
  });
});
