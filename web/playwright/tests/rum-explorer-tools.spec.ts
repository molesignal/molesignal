import { readFile } from 'node:fs/promises';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM, mockRumData } from '../fixtures/mockRumData';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);
const WEEK = 'time=now-7d..now';

test.describe('RUM explorer tools', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('draws how a number falls across the rows, of the number asked for', async ({ page }) => {
    await page.goto(`/rum/explorer?type=views&viz=distribution&${WEEK}`);

    const chart = page.locator('[data-rum-viz-body="distribution"]');
    const measured = DATA.views.filter((row) => typeof row['view.loading_time'] === 'number');

    await expect(chart).toBeVisible();
    await expect(chart.locator('[data-rum-distribution-figure="rows"]')).toHaveText(String(measured.length));
    await expect(chart.locator('[data-rum-distribution-bar]').first()).toBeAttached();
    await expect(page.locator('[data-rum-viz-control="field"]')).toContainText('view.loading_time');

    await page.locator('[data-rum-viz-control="field"]').click();
    await page.getByRole('option', { name: /view\.time_spent/ }).click();

    await expect(page).toHaveURL(/field=view\.time_spent/);
    await expect(page.locator('[data-rum-viz-control="field"]')).toContainText('view.time_spent');
  });

  test('downloads the rows the query matches, with the columns on screen', async ({ page }) => {
    await page.goto(`/rum/explorer?type=errors&${WEEK}`);
    await expect(page.locator('[data-rum-row]').first()).toBeVisible();

    const [download] = await Promise.all([page.waitForEvent('download'), page.locator('[data-rum-export]').click()]);

    expect(download.suggestedFilename()).toMatch(/^molesignal-rum-errors-.*\.csv$/);

    const text = await readFile(await download.path(), 'utf8');
    const lines = text.replace('﻿', '').trim().split('\r\n');

    expect(lines[0]).toMatch(/^time,/);
    expect(lines).toHaveLength(DATA.errors.length + 1);
  });

  test('keeps a view under a name, and opens it again', async ({ page }) => {
    const query = `application.id = '${MOCK_RUM.applications.web}'`;

    await page.goto(`/rum/explorer?type=errors&q=${encodeURIComponent(query)}&${WEEK}`);

    await page.getByRole('button', { name: 'Saved views' }).click();

    const saved = page.locator('[data-rum-saved-views]');

    await expect(saved).toContainText('The views you save are listed here.');

    await saved.getByRole('textbox', { name: 'Name of this view' }).fill('Web errors');
    await saved.getByRole('button', { name: 'Save' }).click();
    await expect(saved.getByRole('button', { name: 'Web errors', exact: true })).toBeVisible();
    await page.keyboard.press('Escape');

    await page.goto(`/rum/explorer?type=views&${WEEK}`);
    await page.getByRole('button', { name: 'Saved views' }).click();
    await saved.getByRole('button', { name: 'Web errors', exact: true }).click();

    await expect(page).toHaveURL(/type=errors/);
    await expect(page).toHaveURL(/q=application\.id/);

    await page.getByRole('button', { name: 'Saved views' }).click();
    await saved.getByRole('button', { name: 'Delete the view Web errors' }).click();
    await expect(saved).toContainText('The views you save are listed here.');
  });
});
