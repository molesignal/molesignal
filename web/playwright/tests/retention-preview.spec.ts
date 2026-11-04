import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const STREAM_PREVIEW = '**/api/v1/settings/stream_retention/preview**';
const AUDIT_PREVIEW = '**/api/v1/settings/audit_retention/preview**';

const NOTHING = {
  retentionDays: 29,
  previousRetentionDays: 30,
  estimated: true,
  generatedAt: 1_800_000_000_000_000,
  rows: 0,
  storedBytes: 0,
  earliestAt: null,
  streamCount: 0,
  streams: [],
  unavailableStreams: 0,
};

const daysField = (page: Page) => page.getByRole('spinbutton', { name: 'Days to keep', exact: true });
const preview = (page: Page) => page.locator('[data-retention-preview]');
const firstSave = (page: Page) => page.getByRole('button', { name: 'Save changes' }).first();
const confirmation = (page: Page) => page.getByRole('dialog');
const shorten = (page: Page) => confirmation(page).getByRole('button', { name: 'Shorten retention' });

const saved = (page: Page, setting: string) =>
  page.waitForRequest(
    (request) => request.method() === 'PUT' && request.url().endsWith(`/settings/${setting}`),
  );

test.describe('retention: what a shorter one would delete', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, {
      scope: 'system',
      platformPermissions: ['sys.settings.retention.read', 'sys.settings.retention.manage'],
      capabilityPermissions: ['sys.settings.retention.read', 'sys.settings.retention.manage'],
    });
  });

  test('shows the impact stream by stream before anything is saved', async ({ page }) => {
    const writes: string[] = [];

    page.on('request', (request) => {
      if (request.method() === 'PUT') writes.push(request.url());
    });

    await page.goto('/settings/stream_retention');
    await expect(daysField(page)).toHaveValue('30');
    await expect(preview(page)).toHaveCount(0);

    await daysField(page).fill('7');

    await expect(preview(page).getByRole('heading', { name: 'What this change would delete' })).toBeVisible();
    await expect(preview(page).getByText('Estimated from the file catalog')).toBeVisible();

    for (const [stream, rows, size] of [
      ['app-logs', '920.0K', '1.35 GiB'],
      ['api-traces', '207.0K', '276.0 MiB'],
      ['host-metrics', '34.5K', '46.0 MiB'],
    ] as const) {
      const row = preview(page).getByRole('row', { name: new RegExp(stream) });

      await expect(row.getByRole('cell').nth(2)).toHaveText(rows);
      await expect(row.getByRole('cell').nth(3)).toHaveText(size);
    }

    // The list leads with the largest, and one organization is not named on every row.
    await expect(preview(page).getByRole('row').nth(1)).toContainText('app-logs');
    await expect(preview(page).getByRole('columnheader', { name: 'Organization' })).toHaveCount(0);
    await expect(preview(page).getByText('Session replays and profile archives')).toBeVisible();
    expect(writes).toEqual([]);
  });

  test('a longer or unchanged retention asks nothing', async ({ page }) => {
    const asked: string[] = [];

    page.on('request', (request) => {
      if (request.url().includes('_retention/preview')) asked.push(request.url());
    });

    await page.goto('/settings/stream_retention');
    await expect(daysField(page)).toHaveValue('30');

    await daysField(page).fill('90');
    await expect(preview(page)).toHaveCount(0);

    const write = saved(page, 'stream_retention');

    await firstSave(page).click();

    expect((await write).postDataJSON()).toEqual({ retentionDays: 90 });
    await expect(confirmation(page)).toHaveCount(0);
    await expect(page.getByText('Settings are in sync').first()).toBeVisible();

    // The field back at what is stored is no change either.
    await daysField(page).fill('90');
    await expect(preview(page)).toHaveCount(0);
    expect(asked).toEqual([]);
  });

  test('a small cut asks for a click, with the facts in the dialog', async ({ page }) => {
    await page.goto('/settings/stream_retention');
    await expect(daysField(page)).toHaveValue('30');

    await daysField(page).fill('29');
    await expect(preview(page).getByRole('row', { name: /app-logs/ })).toBeVisible();
    await firstSave(page).click();

    await expect(confirmation(page).getByRole('heading')).toHaveText(
      'Shorten the default retention from 30 days to 29 days?',
    );

    const facts = confirmation(page).locator('[data-confirm-consequences]');

    await expect(facts).toContainText('Streams');
    await expect(facts).toContainText('50.5K');
    await expect(facts).toContainText('74.0 MiB');
    await expect(confirmation(page)).toContainText('estimated from the file catalog');
    await expect(confirmation(page).getByTestId('confirm-type-name')).toHaveCount(0);

    const write = saved(page, 'stream_retention');

    await shorten(page).click();

    expect((await write).postDataJSON()).toEqual({ retentionDays: 29 });
    await expect(confirmation(page)).toHaveCount(0);
    await expect(preview(page)).toHaveCount(0);
  });

  test('a large cut asks for the days typed back, and Cancel leaves the draft', async ({ page }) => {
    let writes = 0;

    page.on('request', (request) => {
      if (request.method() === 'PUT') writes += 1;
    });

    await page.goto('/settings/stream_retention');
    await expect(daysField(page)).toHaveValue('30');

    await daysField(page).fill('7');
    await expect(preview(page).getByRole('row', { name: /app-logs/ })).toBeVisible();
    await firstSave(page).click();

    await expect(shorten(page)).toBeDisabled();

    await confirmation(page).getByRole('button', { name: 'Cancel' }).click();
    await expect(confirmation(page)).toHaveCount(0);
    await expect(daysField(page)).toHaveValue('7');
    expect(writes).toBe(0);

    await firstSave(page).click();

    const typed = confirmation(page).getByTestId('confirm-type-name');

    await typed.fill('30');
    await expect(shorten(page)).toBeDisabled();
    await typed.fill('7');
    await expect(shorten(page)).toBeEnabled();

    const write = saved(page, 'stream_retention');

    await shorten(page).click();

    expect((await write).postDataJSON()).toEqual({ retentionDays: 7 });
    await expect(confirmation(page)).toHaveCount(0);
  });

  test('Enter goes through the same questions as Save', async ({ page }) => {
    await page.goto('/settings/stream_retention');
    await expect(daysField(page)).toHaveValue('30');

    await daysField(page).fill('7');
    await expect(preview(page).getByRole('row', { name: /app-logs/ })).toBeVisible();
    await daysField(page).press('Enter');

    await expect(confirmation(page).getByTestId('confirm-type-name')).toBeVisible();
  });

  test('nothing to delete says so and saves without asking', async ({ page }) => {
    await page.route(STREAM_PREVIEW, (route) => route.fulfill({ json: NOTHING }));
    await page.goto('/settings/stream_retention');
    await expect(daysField(page)).toHaveValue('30');

    await daysField(page).fill('29');
    await expect(preview(page).getByText('Nothing to delete right now')).toBeVisible();
    await expect(preview(page).getByRole('table')).toHaveCount(0);

    const write = saved(page, 'stream_retention');

    await firstSave(page).click();

    expect((await write).postDataJSON()).toEqual({ retentionDays: 29 });
    await expect(confirmation(page)).toHaveCount(0);
  });

  test('Save waits for the estimate', async ({ page }) => {
    let answer = () => {};
    const held = new Promise<void>((resolve) => {
      answer = resolve;
    });

    await page.route(STREAM_PREVIEW, async (route) => {
      await held;
      await route.fulfill({ json: NOTHING });
    });

    await page.goto('/settings/stream_retention');
    await expect(daysField(page)).toHaveValue('30');

    await daysField(page).fill('29');
    await expect(preview(page).getByText('Estimating what this change would delete…')).toBeVisible();
    await expect(firstSave(page)).toBeDisabled();

    answer();
    await expect(firstSave(page)).toBeEnabled();
  });

  test('an estimate that fails says so, and Save asks for the days typed', async ({ page }) => {
    let failing = true;

    await page.route(STREAM_PREVIEW, async (route) => {
      if (failing) await route.fulfill({ status: 503, json: { error: 'catalog unavailable' } });
      else await route.fallback();
    });

    await page.goto('/settings/stream_retention');
    await expect(daysField(page)).toHaveValue('30');

    await daysField(page).fill('29');
    await expect(preview(page).getByRole('alert')).toContainText('The impact could not be estimated');
    await firstSave(page).click();

    // A figure it does not have is not drawn as a figure.
    await expect(confirmation(page).locator('[data-confirm-consequences]')).toHaveCount(0);
    await expect(confirmation(page)).toContainText('The impact could not be estimated');
    await expect(confirmation(page).getByTestId('confirm-type-name')).toBeVisible();
    await confirmation(page).getByRole('button', { name: 'Cancel' }).click();

    failing = false;
    await preview(page).getByRole('button', { name: 'Retry' }).click();
    await expect(preview(page).getByRole('row', { name: /app-logs/ })).toBeVisible();
  });

  test('streams that could not be read count against the figures', async ({ page }) => {
    await page.route(STREAM_PREVIEW, (route) =>
      route.fulfill({
        json: {
          ...NOTHING,
          rows: 10,
          storedBytes: 100,
          earliestAt: 1_790_000_000_000_000,
          streamCount: 1,
          streams: [
            {
              orgId: 'acme-prod',
              orgName: 'Acme Production',
              id: 's-app',
              name: 'app-logs',
              streamType: 'logs',
              rows: 10,
              storedBytes: 100,
              earliestAt: 1_790_000_000_000_000,
            },
          ],
          unavailableStreams: 2,
        },
      }),
    );

    await page.goto('/settings/stream_retention');
    await expect(daysField(page)).toHaveValue('30');

    await daysField(page).fill('29');
    await expect(preview(page).getByText('2 streams, so the real figures may be higher')).toBeVisible();
    await firstSave(page).click();

    await expect(confirmation(page).locator('[data-confirm-consequences]')).toContainText('2 streams');
    await expect(confirmation(page).getByTestId('confirm-type-name')).toBeVisible();
  });

  test('several organizations are named, and a long list opens on request', async ({ page }) => {
    const streams = Array.from({ length: 12 }, (_, index) => ({
      orgId: index % 2 === 0 ? 'acme-prod' : 'acme-staging',
      orgName: index % 2 === 0 ? 'Acme Production' : 'Acme Staging',
      id: `s-${index}`,
      name: `stream-${String(index).padStart(2, '0')}`,
      streamType: 'logs',
      rows: 10,
      storedBytes: 100,
      earliestAt: 1_790_000_000_000_000,
    }));

    await page.route(STREAM_PREVIEW, (route) =>
      route.fulfill({
        json: {
          ...NOTHING,
          rows: 120,
          storedBytes: 1200,
          earliestAt: 1_790_000_000_000_000,
          streamCount: 12,
          streams,
        },
      }),
    );

    await page.goto('/settings/stream_retention');
    await expect(daysField(page)).toHaveValue('30');

    await daysField(page).fill('29');
    await expect(preview(page).getByRole('columnheader', { name: 'Organization' })).toBeVisible();
    await expect(preview(page).getByRole('row')).toHaveCount(11);
    await expect(preview(page).getByText('The largest 10 of 12 streams.')).toBeVisible();

    await preview(page).getByRole('button', { name: 'Show more' }).click();
    await expect(preview(page).getByRole('row')).toHaveCount(13);
    await expect(preview(page).getByText('The largest 10 of 12 streams.')).toHaveCount(0);
  });

  test('the runs of synthetic checks keep their own plain save', async ({ page }) => {
    await page.goto('/settings/stream_retention');

    const runs = page.getByRole('spinbutton', { name: 'Days to keep runs' });

    await expect(runs).toHaveValue('30');
    await runs.fill('7');
    await expect(preview(page)).toHaveCount(0);

    const write = saved(page, 'synthetic_retention');

    await page.getByRole('button', { name: 'Save changes' }).nth(1).click();

    expect((await write).postDataJSON()).toEqual({ retentionDays: 7 });
    await expect(confirmation(page)).toHaveCount(0);
  });
});

test.describe('audit retention: what a shorter one would delete', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, {
      scope: 'system',
      platformPermissions: ['sys.settings.retention.read', 'sys.settings.retention.manage'],
      capabilityPermissions: ['sys.settings.retention.read', 'sys.settings.retention.manage'],
    });
  });

  test('counts the records, and many of them ask for the days typed', async ({ page }) => {
    await page.goto('/settings/audit_retention');
    await expect(daysField(page)).toHaveValue('90');

    await daysField(page).fill('30');

    // Counted, not estimated: 60 days at 40 records a day.
    await expect(preview(page)).toContainText('This change would delete 2,400 records, the oldest from');

    await firstSave(page).click();

    await expect(confirmation(page).getByRole('heading')).toHaveText(
      'Shorten audit retention from 90 days to 30 days?',
    );
    await expect(confirmation(page).locator('[data-confirm-consequences]')).toContainText('2,400');
    await expect(shorten(page)).toBeDisabled();

    await confirmation(page).getByTestId('confirm-type-name').fill('30');

    const write = saved(page, 'audit_retention');

    await shorten(page).click();

    expect((await write).postDataJSON()).toEqual({ retentionDays: 30 });
    await expect(confirmation(page)).toHaveCount(0);
  });

  test('a few records ask for a click', async ({ page }) => {
    await page.goto('/settings/audit_retention');
    await expect(daysField(page)).toHaveValue('90');

    await daysField(page).fill('89');
    await expect(preview(page)).toContainText('40 records');
    await firstSave(page).click();

    await expect(confirmation(page).getByTestId('confirm-type-name')).toHaveCount(0);
    await expect(shorten(page)).toBeEnabled();
  });

  test('keeping records for good and for longer asks nothing', async ({ page }) => {
    const asked: string[] = [];

    page.on('request', (request) => {
      if (request.url().includes('_retention/preview')) asked.push(request.url());
    });

    await page.goto('/settings/audit_retention');
    await expect(daysField(page)).toHaveValue('90');

    await daysField(page).fill('365');
    await expect(preview(page)).toHaveCount(0);

    await daysField(page).fill('0');
    await expect(preview(page)).toHaveCount(0);

    const write = saved(page, 'audit_retention');

    await firstSave(page).click();

    expect((await write).postDataJSON()).toEqual({ retentionDays: 0 });
    await expect(confirmation(page)).toHaveCount(0);
    expect(asked).toEqual([]);
  });

  test('from keeping for good, a retention deletes everything older', async ({ page, mockServer }) => {
    await fetch(`http://127.0.0.1:${mockServer.port}/api/v1/settings/audit_retention`, {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ retentionDays: 0 }),
    });

    await page.goto('/settings/audit_retention');
    await expect(daysField(page)).toHaveValue('0');

    await daysField(page).fill('90');
    await expect(preview(page)).toContainText('This change would delete');
    await firstSave(page).click();

    await expect(confirmation(page).getByRole('heading')).toHaveText(
      'Stop keeping audit records forever and keep them for 90 days?',
    );
  });

  test('records that cannot be counted say so and ask for the days typed', async ({ page }) => {
    await page.route(AUDIT_PREVIEW, (route) => route.fulfill({ status: 503, json: { error: 'unavailable' } }));

    await page.goto('/settings/audit_retention');
    await expect(daysField(page)).toHaveValue('90');

    await daysField(page).fill('89');
    await expect(preview(page).getByRole('alert')).toContainText('could not be counted');
    await firstSave(page).click();

    await expect(confirmation(page).getByTestId('confirm-type-name')).toBeVisible();
  });
});
