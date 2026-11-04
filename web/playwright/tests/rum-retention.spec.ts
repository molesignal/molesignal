import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { issueIdOf, MOCK_RUM } from '../fixtures/mockRumData';

const WEB = MOCK_RUM.applications.web;
const TYPE_ERROR = issueIdOf(WEB, 'TypeError', "Cannot read properties of undefined (reading 'price')");

const rowOf = (page: Page, id: string) => page.getByRole('row').filter({ has: page.getByText(id, { exact: true }) });

const saved = (page: Page) =>
  page.waitForRequest((request) => request.method() === 'PATCH' && request.url().endsWith('/rum/applications/storefront'));

interface SentFilter {
  id: string;
  name: string;
  rate: number;
  conditions: Array<{ field: string; op: string; values: string[] }>;
}

async function openDrawer(page: Page) {
  await page.goto('/rum/applications');
  await rowOf(page, 'storefront').click();

  return page.getByRole('dialog');
}

test.describe('RUM retention: the settings of an application', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('starts with no filter, and says that every session is kept by the share', async ({ page }) => {
    const drawer = await openDrawer(page);

    await expect(drawer.getByText('No filters: every session is kept by the share below.')).toBeVisible();
    await expect(drawer.locator('[data-rum-retention-filter]')).toHaveCount(0);
    await expect(drawer.getByRole('spinbutton', { name: 'Share of sessions kept (%)' })).toHaveValue('100');
    await expect(drawer.getByRole('switch', { name: 'Keep every error' })).toBeChecked();
  });

  test('adds a filter with a share of its own and sends it as it stands', async ({ page }) => {
    const drawer = await openDrawer(page);

    await drawer.getByRole('button', { name: 'Add a filter' }).click();

    const card = drawer.locator('[data-rum-retention-filter]');

    await expect(card).toHaveCount(1);

    // The filter is named, and compares the environment with nothing yet: it is not one to save.
    await expect(card.getByRole('textbox', { name: 'Filter name' })).toHaveValue('Filter 1');
    await expect(drawer.getByRole('button', { name: 'Save', exact: true })).toBeDisabled();

    // The share of the others is named by what it is now.
    await expect(drawer.getByRole('spinbutton', { name: 'Share of the other sessions kept (%)' })).toHaveValue('100');

    await card.getByRole('textbox', { name: 'Filter name' }).fill('Production');
    await card.getByRole('spinbutton', { name: 'Share of its sessions kept (%)' }).fill('100');
    await card.getByRole('textbox', { name: 'Values' }).fill('prod, production');
    await drawer.getByRole('spinbutton', { name: 'Share of the other sessions kept (%)' }).fill('10');

    await expect(drawer.getByRole('button', { name: 'Save', exact: true })).toBeEnabled();

    const request = saved(page);

    await drawer.getByRole('button', { name: 'Save', exact: true }).click();

    const body = (await request).postDataJSON() as { settings: { retentionFilters: SentFilter[]; sessionSampleRate: number; keepErrors: boolean } };

    // A filter that is new has no id: the server makes one from the name.
    expect(body.settings.retentionFilters).toEqual([
      { id: '', name: 'Production', rate: 100, conditions: [{ field: 'env', op: 'is', values: ['prod', 'production'] }] },
    ]);
    expect(body.settings.sessionSampleRate).toBe(10);
    expect(body.settings.keepErrors).toBe(true);

    // What was saved is what the drawer shows when it is opened again, the filter named by the server.
    await drawer.getByRole('button', { name: 'Close', exact: true }).last().click();

    const again = await (async () => {
      await rowOf(page, 'storefront').click();

      return page.getByRole('dialog');
    })();

    await expect(again.locator('[data-rum-retention-filter]')).toHaveCount(1);
    await expect(again.getByRole('textbox', { name: 'Filter name' })).toHaveValue('Production');
    await expect(again.getByRole('textbox', { name: 'Values' })).toHaveValue('prod, production');
    await expect(again.getByRole('spinbutton', { name: 'Share of the other sessions kept (%)' })).toHaveValue('10');
  });

  test('compares what a session is like with values: its country, its browser, and what it is not', async ({ page }) => {
    const drawer = await openDrawer(page);

    await drawer.getByRole('button', { name: 'Add a filter' }).click();

    const card = drawer.locator('[data-rum-retention-filter]');

    await card.getByRole('combobox', { name: 'What the condition looks at' }).click();
    await page.getByRole('option', { name: 'Country code' }).click();
    await card.getByRole('combobox', { name: 'How it compares' }).click();
    await page.getByRole('option', { name: 'is not' }).click();
    await card.getByRole('textbox', { name: 'Values' }).fill('CN，HK');

    // A second condition: both have to hold.
    await card.getByRole('button', { name: 'Add a condition' }).click();
    await expect(card.locator('[data-rum-retention-condition]')).toHaveCount(2);
    await card.locator('[data-rum-retention-condition]').nth(1).getByRole('textbox', { name: 'Values' }).fill('prod');
    await card.getByRole('spinbutton', { name: 'Share of its sessions kept (%)' }).fill('20');

    const request = saved(page);

    await drawer.getByRole('button', { name: 'Save', exact: true }).click();

    const body = (await request).postDataJSON() as { settings: { retentionFilters: SentFilter[] } };

    expect(body.settings.retentionFilters[0]).toMatchObject({
      rate: 20,
      conditions: [
        { field: 'geo.country_iso_code', op: 'is_not', values: ['CN', 'HK'] },
        { field: 'env', op: 'is', values: ['prod'] },
      ],
    });
  });

  test('puts the filters in the order they are asked in, and takes one out', async ({ page }) => {
    const drawer = await openDrawer(page);
    const names = drawer.getByRole('textbox', { name: 'Filter name' });

    for (const name of ['Alpha', 'Beta', 'Gamma']) {
      await drawer.getByRole('button', { name: 'Add a filter' }).click();
      await names.last().fill(name);
      await drawer.locator('[data-rum-retention-filter]').last().getByRole('textbox', { name: 'Values' }).fill('x');
    }

    // The first that describes a session decides: Gamma goes first, Beta goes.
    await drawer.getByRole('button', { name: 'Move this filter up' }).nth(2).click();
    await drawer.getByRole('button', { name: 'Move this filter up' }).nth(1).click();
    await expect(names).toHaveCount(3);
    await expect(names.nth(0)).toHaveValue('Gamma');
    await expect(names.nth(1)).toHaveValue('Alpha');

    await drawer.getByRole('button', { name: 'Remove this filter' }).nth(2).click();
    await expect(names).toHaveCount(2);

    const request = saved(page);

    await drawer.getByRole('button', { name: 'Save', exact: true }).click();

    const body = (await request).postDataJSON() as { settings: { retentionFilters: SentFilter[] } };

    expect(body.settings.retentionFilters.map((filter) => filter.name)).toEqual(['Gamma', 'Alpha']);
  });

  test('does not save two filters of one name, a filter with no values, or a share beyond all of them', async ({ page }) => {
    const drawer = await openDrawer(page);
    const save = drawer.getByRole('button', { name: 'Save', exact: true });

    for (let count = 0; count < 2; count += 1) {
      await drawer.getByRole('button', { name: 'Add a filter' }).click();
      await drawer.locator('[data-rum-retention-filter]').last().getByRole('textbox', { name: 'Values' }).fill('prod');
    }

    await expect(save).toBeEnabled();

    await drawer.getByRole('textbox', { name: 'Filter name' }).nth(1).fill('filter 1');
    await expect(save).toBeDisabled();
    await expect(drawer.getByRole('textbox', { name: 'Filter name' }).nth(1)).toHaveAttribute('aria-invalid', 'true');

    await drawer.getByRole('textbox', { name: 'Filter name' }).nth(1).fill('Other');
    await expect(save).toBeEnabled();

    await drawer.getByRole('textbox', { name: 'Values' }).first().fill('');
    await expect(save).toBeDisabled();
    await drawer.getByRole('textbox', { name: 'Values' }).first().fill('prod');
    await expect(save).toBeEnabled();

    await drawer.getByRole('spinbutton', { name: 'Share of its sessions kept (%)' }).first().fill('150');
    await expect(save).toBeDisabled();
  });

  test('stops adding filters at the most an application has', async ({ page }) => {
    const drawer = await openDrawer(page);

    for (let count = 0; count < 10; count += 1) await drawer.getByRole('button', { name: 'Add a filter' }).click();

    await expect(drawer.locator('[data-rum-retention-filter]')).toHaveCount(10);
    await expect(drawer.getByRole('button', { name: 'Add a filter' })).toHaveCount(0);
  });

  test('can stop keeping the errors of the sessions that are not kept', async ({ page }) => {
    const drawer = await openDrawer(page);

    await drawer.getByRole('switch', { name: 'Keep every error' }).click();

    const request = saved(page);

    await drawer.getByRole('button', { name: 'Save', exact: true }).click();

    const body = (await request).postDataJSON() as { settings: { keepErrors: boolean } };

    expect(body.settings.keepErrors).toBe(false);
  });
});

test.describe('RUM retention: what the pages say', () => {
  /** The read of the note: the sessions as many as they stand for, and as many as are stored. */
  async function answerKept(page: Page, estimated: number, stored: number) {
    await page.route(/\/api\/v1\/rum\/aggregate/, async (route) => {
      const asked = decodeURIComponent(route.request().url() + (route.request().postData() ?? ''));

      if (!asked.includes('"stored"')) {
        await route.fallback();

        return;
      }

      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({
          groupBy: [],
          measures: [
            { name: 'estimated', fn: 'count', unit: 'count' },
            { name: 'stored', fn: 'stored', unit: 'count' },
          ],
          rows: [{ group: [], values: [estimated, stored] }],
        }),
      });
    });
  }

  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('says that the totals are estimates when only a share of the sessions was kept', async ({ page }) => {
    await answerKept(page, 4_000, 1_000);

    for (const path of ['/rum/overview', '/rum/errors', '/rum/performance', '/rum/frustration', '/rum/analytics']) {
      await page.goto(path);

      await expect(page.locator('[data-rum-estimate]'), path).toHaveText('Estimated · 25% of sessions kept');
    }

    await page.locator('[data-rum-estimate]').hover();
    await expect(page.getByRole('tooltip')).toContainText('Only 25% of the sessions in this range were kept.');
  });

  test('says nothing while every session is kept', async ({ page }) => {
    await page.goto('/rum/overview');

    await expect(page.locator('[data-rum-scope="application"]')).toBeVisible();
    await expect(page.locator('[data-rum-estimate]')).toHaveCount(0);

    await answerKept(page, 1_000, 1_000);
    await page.goto('/rum/errors');
    await expect(page.locator('[data-rum-scope="application"]')).toBeVisible();
    await expect(page.locator('[data-rum-estimate]')).toHaveCount(0);
  });

  test('has no session to open for an error whose session was not kept', async ({ page, mockServer }) => {
    await page.route(new RegExp(`/api/v1/rum/issues/${TYPE_ERROR}`), async (route) => {
      const url = new URL(route.request().url());
      const response = await route.fetch({ url: `http://127.0.0.1:${mockServer.port}${url.pathname}${url.search}` });
      const body = (await response.json()) as { samples: Array<Record<string, unknown>> };

      await route.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ ...body, samples: body.samples.map((sample) => ({ ...sample, retained: false })) }),
      });
    });

    await page.goto(`/rum/errors?issue=${TYPE_ERROR}`);

    const sample = page.locator('[data-rum-issue-drawer] [data-rum-sample]');

    await expect(sample).toHaveCount(1);
    await expect(sample.locator('[data-rum-session-not-kept]')).toHaveText('Session not kept');
    await expect(sample.getByRole('link')).toHaveCount(0);
  });
});

test.describe('RUM retention in Chinese', () => {
  test('says its words in the language of the product', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await page.goto('/rum/applications');
    await rowOf(page, 'storefront').click();

    const drawer = page.getByRole('dialog');

    await expect(drawer.getByText('没有过滤器：所有会话都按下面的比例保留。')).toBeVisible();
    await expect(drawer.getByRole('switch', { name: '保留所有错误' })).toBeChecked();

    await drawer.getByRole('button', { name: '添加过滤器' }).click();

    const card = drawer.locator('[data-rum-retention-filter]');

    await expect(card.getByRole('textbox', { name: '过滤器名称' })).toHaveValue('过滤器 1');
    await expect(card.getByRole('combobox', { name: '条件看什么' })).toContainText('环境');
    await expect(card.getByRole('combobox', { name: '怎么比较' })).toContainText('是');
    await expect(drawer.getByRole('spinbutton', { name: '其余会话的保留比例（%）' })).toHaveValue('100');
  });
});
