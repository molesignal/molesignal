import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM } from '../fixtures/mockRumData';

const WEB = MOCK_RUM.applications.web;

/**
 * Opens the page and waits for the time range the page puts in the address, as it does 80 ms after
 * it mounts: a click made before that would be written over by it.
 */
async function open(page: Page, path: string) {
  await page.goto(path);
  await expect(page).toHaveURL(/[?&]time=/);
}

/** The bodies sent to a path, after which the request goes on to the mock server. */
async function sent(page: Page, path: string, method: 'POST' | 'PUT'): Promise<Array<Record<string, unknown>>> {
  const bodies: Array<Record<string, unknown>> = [];

  await page.route(path, async (route) => {
    if (route.request().method() === method) bodies.push(route.request().postDataJSON() as Record<string, unknown>);

    await route.fallback();
  });

  return bodies;
}

const explorer = (search: Record<string, string | string[]>) => {
  const params = new URLSearchParams();

  for (const [key, value] of Object.entries(search)) {
    for (const one of Array.isArray(value) ? value : [value]) params.append(key, one);
  }

  return `/rum/explorer?${params}`;
};

const dialogOf = (page: Page) => page.getByRole('dialog', { name: 'Add to dashboard' });

test.describe('RUM explorer: what is drawn on a dashboard and in an alert rule', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('a chart over time goes to a new dashboard that cuts it by its own interval', async ({ page }) => {
    const asked = await sent(page, '**/api/v1/rum/panel-statement', 'POST');
    const created = await sent(page, '**/api/v1/dashboards', 'POST');

    await open(page, explorer({ type: 'views', viz: 'timeseries', m: 'p75:view.loading_time', by: 'browser.name', q: "env = 'prod'" }));
    await page.locator('[data-rum-add-to-dashboard]').click();

    const dialog = dialogOf(page);

    await expect(dialog.locator('[data-rum-dashboard-shape="timeseries"]')).toContainText('Drawn as a time series');
    await expect(dialog.locator('[data-rum-dashboard-interval]')).toContainText('Interval variable');
    await expect(dialog.getByLabel('Panel title')).toHaveValue('p75 Loading by Browser');
    await dialog.getByLabel('Dashboard name').fill('Checkout speed');
    await dialog.locator('[data-rum-dashboard-confirm]').click();

    await expect.poll(() => created.length).toBe(1);

    // The panel is asked for the explorer's query, measure and groups, cut by the dashboard's interval.
    expect(asked[0]).toMatchObject({
      type: 'views',
      chart: 'timeseries',
      filters: [{ field: 'env', op: '=', value: 'prod' }],
      measures: [{ fn: 'p75', field: 'view.loading_time', name: 'p75 Loading' }],
      groupBy: ['browser.name'],
      groupNames: ['Browser'],
      interval: '${interval}',
    });
    expect(created[0]!.model).toMatchObject({
      title: 'Checkout speed',
      tags: ['rum'],
      variables: [{ name: 'interval', type: 'interval' }],
      elements: [
        {
          kind: 'panel',
          title: 'p75 Loading by Browser',
          visualization: { type: 'time_series', options: { drawStyle: 'line', legendMode: 'list' } },
          fieldConfig: { unit: 'milliseconds' },
          queries: [
            {
              dataSourceType: 'sql',
              format: 'time_series',
              query: { language: 'sql', streamName: 'rum_views', streamType: 'logs', limit: 10_000 },
            },
          ],
        },
      ],
    });

    const query = ((created[0]!.model as { elements: Array<{ queries: Array<{ query: { statement: string } }> }> }).elements[0]!
      .queries[0]!.query.statement);

    expect(query).toContain('FROM "rum_views_latest"');
    expect(query).toContain("\"env\" = 'prod'");
    await expect(dialog).toHaveCount(0);
    await expect(page.getByText('Created dashboard “Checkout speed”')).toBeVisible();
  });

  test('a top list goes under what a dashboard there is has, with no interval to add', async ({ page }) => {
    const updated = await sent(page, '**/api/v1/dashboards/d1', 'PUT');

    await open(page, explorer({ type: 'views', viz: 'toplist', m: 'count', by: 'view.url_path_group' }));
    await page.locator('[data-rum-add-to-dashboard]').click();

    const dialog = dialogOf(page);

    await dialog.locator('[data-rum-dashboard-target="existing"]').click();
    await dialog.getByRole('combobox', { name: 'Dashboard', exact: true }).click();
    await page.getByRole('option', { name: 'Web overview', exact: true }).click();
    await expect(dialog.locator('[data-rum-dashboard-interval]')).toHaveCount(0);
    await dialog.locator('[data-rum-dashboard-confirm]').click();

    await expect.poll(() => updated.length).toBe(1);

    const model = updated[0]!.model as { title: string; variables: unknown[]; elements: Array<Record<string, unknown>> };

    expect(model.title).toBe('Web overview');
    expect(model.variables).toEqual([]);
    expect(model.elements.at(-1)).toMatchObject({
      kind: 'panel',
      visualization: { type: 'bar_chart', options: { orientation: 'horizontal' } },
      fieldConfig: { unit: 'short' },
    });
    await expect(page.getByText('Added to dashboard “Web overview”')).toBeVisible();
  });

  test('the dialog says what a list and a map become on a dashboard', async ({ page }) => {
    await open(page, explorer({ type: 'views' }));
    await page.locator('[data-rum-add-to-dashboard]').click();
    await expect(dialogOf(page).locator('[data-rum-dashboard-notes]')).toContainText('like the histogram above it');
    await page.keyboard.press('Escape');

    await open(page, explorer({ type: 'sessions', viz: 'map' }));
    await page.locator('[data-rum-add-to-dashboard]').click();
    await expect(dialogOf(page).locator('[data-rum-dashboard-notes]')).toContainText('Dashboards have no map');
  });

  test('a new alert says what the rule leaves behind and opens the editor on the query and its first number', async ({ page }) => {
    const statement = `application.id = '${WEB}' and env = 'prod'`;

    await open(
      page,
      explorer({ type: 'views', viz: 'table', m: ['p75:view.loading_time', 'count'], by: 'browser.name', q: statement }),
    );
    await page.locator('[data-rum-new-alert]').click();

    const menu = page.locator('[data-rum-new-alert-menu]');

    await expect(menu.locator('[data-rum-alert-watches]')).toHaveText('The rule watches: p75 Loading.');
    await expect(menu.locator('[data-rum-alert-left]')).toHaveText(
      'Not carried over: the way it is drawn (Table), the groups (Browser), the other measures (Count of views).',
    );
    // The query names the application, so the rule is about it.
    await expect(menu.locator('[data-rum-alert-application]')).toHaveCount(0);

    await menu.locator('[data-rum-new-alert-kind="threshold"]').click();
    await expect(page).toHaveURL(/\/alerts\/rules\/new\?source=rum&app=storefront&metric=query/);

    const source = page.locator('[data-alert-rum-source]');
    const combobox = (name: string) => source.getByRole('combobox', { name, exact: true });

    await expect(combobox('Application')).toContainText('Storefront');
    await expect(combobox('Environment')).toContainText('prod');
    await expect(combobox('Figure')).toContainText('Explorer query');
    await expect(combobox('Type of rows')).toContainText('Views');
    await expect(combobox('Measure')).toContainText('p75');
    await expect(combobox('Field')).toContainText('view.loading_time');
    await expect(source.locator('[data-alert-rum-statement]')).toHaveValue(statement);
    // A query names its page and its version itself.
    await expect(combobox('Page')).toBeDisabled();
    await expect(source).toContainText('Name the page in the query.');
    await expect(source.locator('[data-alert-rum-explorer]')).toHaveAttribute('href', /\/rum\/explorer\?type=views&q=.*&viz=timeseries&m=p75%3Aview\.loading_time/);
  });

  test('without an application in the query, the rule asks for one in the editor', async ({ page }) => {
    await open(page, explorer({ type: 'errors' }));
    await page.locator('[data-rum-new-alert]').click();

    const menu = page.locator('[data-rum-new-alert-menu]');

    await expect(menu.locator('[data-rum-alert-watches]')).toHaveText('The rule watches: Count of errors.');
    await expect(menu.locator('[data-rum-alert-left]')).toHaveCount(0);
    await expect(menu.locator('[data-rum-alert-application]')).toContainText('choose it in the editor');
  });
});
