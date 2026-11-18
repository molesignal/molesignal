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

  test('opens a row right under the top bar, its stack in the colours of code', async ({ page }) => {
    await page.goto(`/rum/explorer?type=errors&${WEEK}`);
    await page.locator('[data-rum-row]').first().click();

    const stack = page.locator('[data-rum-stack]');

    await expect(stack).toBeVisible();
    // The error, the functions and where they are, each read apart from the words between them.
    await expect(stack.locator('[data-stack-part="type"]').first()).toHaveText('TypeError');
    await expect(stack.locator('[data-stack-part="function"]').first()).toHaveText('renderTotals');
    await expect(stack.locator('[data-stack-part="position"]').first()).toHaveText(':1:20431');

    // The drawer begins where the top bar ends: no strip of the page shows between them. The bar is
    // hidden from assistive technology while the drawer is open, so it is found by its markup.
    const bar = (await page.locator('header[role="banner"]').boundingBox())!;
    const drawer = (await page.getByRole('dialog').boundingBox())!;

    expect(Math.abs(drawer.y - (bar.y + bar.height))).toBeLessThanOrEqual(0.5);
  });

  test('lists the fields of one name together, wherever some of them are indexed', async ({ page }) => {
    await page.goto(`/rum/explorer?type=sessions&${WEEK}`);

    // `synthetics.test_id` and `.result_id` are indexed, `.location_id` is not: one group holds all three.
    const synthetics = page.locator('[data-field-group="synthetics"]');

    await expect(synthetics).toHaveCount(1);
    await expect(synthetics.locator('[data-field-group-count]')).toHaveText('3');
  });

  test('opens a row on what it says first, as a log does: its numbers, where it happened, then the rest of its fields', async ({ page }) => {
    await page.goto(`/rum/explorer?type=sessions&${WEEK}`);
    const rows = page.locator('[data-rum-row]');
    await rows.first().click();
    const drawer = page.locator('[data-rum-drawer]');
    const total = await rows.count();

    // The numbers a session is read by, each in a tile; who it was for, in a pill.
    await expect(drawer.locator('[data-rum-drawer-tile="session.duration"]')).toBeVisible();
    await expect(drawer.locator('[data-rum-drawer-pill="usr.id"]')).toBeVisible();
    // What is said above is not said again in the list of fields.
    const listed = drawer.locator('[data-rum-drawer-fields]');
    await expect(listed.locator('[data-rum-drawer-field="session.id"]')).toBeVisible();
    await expect(listed.locator('[data-rum-drawer-field="session.duration"]')).toHaveCount(0);
    await expect(listed.locator('[data-rum-drawer-field="usr.id"]')).toHaveCount(0);
    // The head says where the row stands in the list, and its marks are as small as those of a log's drawer.
    await expect(drawer.getByLabel(`Row 1 of ${total}`)).toHaveText(`1 / ${total}`);
    expect((await drawer.getByRole('button', { name: 'Close the details' }).locator('svg').boundingBox())!.width).toBe(12);

    // The JSON fills the room under the tabs, down to the foot of the drawer, however short the row is.
    await drawer.locator('[data-rum-drawer-sections]').getByRole('button', { name: 'JSON' }).click();
    const room = drawer.locator('[data-rum-drawer-json]');
    await expect(room.locator('.monaco-editor')).toBeVisible();
    const [roomBox, editorBox] = await Promise.all([room.boundingBox(), room.locator(':scope > *').first().boundingBox()]);
    expect(Math.abs(editorBox!.y + editorBox!.height - (roomBox!.y + roomBox!.height))).toBeLessThanOrEqual(1);
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

  test('draws no distribution of when a session was last active: it is a moment, not an amount', async ({ page }) => {
    await page.goto(`/rum/explorer?type=sessions&viz=distribution&field=session.last_activity&${WEEK}`);

    const field = page.locator('[data-rum-viz-control="field"]');

    // Asked for by the address, it gives way to how long the sessions lasted.
    await expect(field).toContainText('session.duration');
    await expect(page.locator('[data-rum-viz-body="distribution"]')).toBeVisible();

    await field.click();
    await expect(page.getByRole('option', { name: /session\.duration/ })).toBeVisible();
    await expect(page.getByRole('option', { name: /session\.last_activity/ })).toHaveCount(0);
  });
});
