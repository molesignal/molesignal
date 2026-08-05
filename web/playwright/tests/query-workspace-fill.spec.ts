import type { Locator, Page } from '@playwright/test';
import { expect, test } from '@playwright/test';

import { installMockShellSession } from '../fixtures/mockSession';

async function boundingBox(locator: Locator) {
  const box = await locator.boundingBox();
  expect(box).not.toBeNull();
  return box!;
}

async function expectEmptyResultsToFillViewport(
  page: Page,
  workspaceName: 'logs' | 'traces',
  paneName: 'log-results' | 'trace-results',
) {
  const workspace = page.locator(`[data-workspace="${workspaceName}"]`);
  const pane = workspace.locator(`[data-workspace-pane="${paneName}"]`);
  const emptyState = pane.getByRole('status');
  const emptyRegion = emptyState.locator('..');
  const pagination = pane.getByRole('navigation');

  await expect(workspace).toBeVisible();
  await expect(emptyState).toBeVisible();
  await expect(pagination).toBeVisible();

  const viewport = page.viewportSize();
  expect(viewport).not.toBeNull();
  const [workspaceBox, paneBox, emptyBox, emptyRegionBox, paginationBox] = await Promise.all([
    boundingBox(workspace),
    boundingBox(pane),
    boundingBox(emptyState),
    boundingBox(emptyRegion),
    boundingBox(pagination),
  ]);

  expect(Math.abs(workspaceBox.y + workspaceBox.height - viewport!.height)).toBeLessThanOrEqual(1);
  expect(Math.abs(paneBox.y + paneBox.height - workspaceBox.y - workspaceBox.height)).toBeLessThanOrEqual(1);
  expect(Math.abs(paginationBox.y + paginationBox.height - paneBox.y - paneBox.height)).toBeLessThanOrEqual(1);
  expect(Math.abs(emptyBox.y - emptyRegionBox.y)).toBeLessThanOrEqual(1);
  expect(Math.abs(emptyBox.height - emptyRegionBox.height)).toBeLessThanOrEqual(1);
}

test.beforeEach(async ({ page }) => {
  await installMockShellSession(page);
  await page.route('**/api/v1/streams**', (route) =>
    route.fulfill({ json: { items: [] } }),
  );
  await page.route('**/api/v1/web/topology**', (route) =>
    route.fulfill({ json: { nodes: [], edges: [] } }),
  );
  await page.route('**/api/v1/web/traces**', (route) =>
    route.fulfill({ json: { items: [] } }),
  );
  await page.route('**/api/v1/query**', (route) =>
    route.fulfill({
      json: { columns: [], rows: [], scanned_rows: 0, took_ms: 0 },
    }),
  );
});

test('trace span and table empty states fill the remaining viewport', async ({ page }) => {
  await page.goto('/traces');
  await expectEmptyResultsToFillViewport(page, 'traces', 'trace-results');

  await page.goto('/traces?tab=traces');
  await expectEmptyResultsToFillViewport(page, 'traces', 'trace-results');
});

test('log empty state fills the remaining viewport', async ({ page }) => {
  await page.goto('/logs');
  await expectEmptyResultsToFillViewport(page, 'logs', 'log-results');
});
