/**
 * What the app does when things go wrong: a page that throws, a list the server
 * could not give, a draft about to be lost, a page the role does not include.
 */
import type { Page, Route } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const FAILURE = {
  status: 500,
  contentType: 'application/json',
  headers: { 'x-request-id': 'req_7c41e9a2' },
  body: JSON.stringify({ code: 'internal', message: 'query engine unavailable' }),
};

async function failUntilAllowed(page: Page, url: string) {
  const state = { failing: true };
  await page.route(url, (route: Route) => (state.failing ? route.fulfill(FAILURE) : route.fallback()));
  return state;
}

test.describe('a page that throws', () => {
  test('keeps the shell, says what happened and goes away on the next page', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    // Not the array the page expects: it throws while drawing.
    await page.route('**/api/v1/domains', (route) => route.fulfill({ json: {} }));

    await page.goto('/settings/domain_management');

    const failure = page.getByTestId('route-error');
    await expect(failure).toBeVisible();
    await expect(failure.getByText('This page ran into a problem')).toBeVisible();
    await expect(failure.getByRole('button', { name: 'Reload this page' })).toBeVisible();
    await expect(failure.getByRole('button', { name: 'Copy error report' })).toBeVisible();
    await expect(page.getByTestId('sidebar-toggle')).toBeVisible();

    await failure.getByRole('link', { name: 'Back to Home' }).click();
    await expect(page).toHaveURL(/\/home(?:[?#]|$)/);
    await expect(page.getByTestId('route-error')).toHaveCount(0);
  });
});

test.describe('a list the server could not give', () => {
  test('says so with its request id, offers a retry, and counts nothing as zero', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    const server = await failUntilAllowed(page, '**/api/v1/scheduled_pipelines');

    await page.goto('/pipelines');

    const failure = page.getByRole('alert').filter({ hasText: 'query engine unavailable' });
    await expect(failure).toBeVisible();
    await expect(failure.getByText('HTTP 500')).toBeVisible();
    await expect(failure.getByText('req_7c41e9a2')).toBeVisible();
    await expect(page.getByText('Running', { exact: true }).locator('xpath=following-sibling::*[1]')).toHaveText('—');

    server.failing = false;
    await failure.getByRole('button', { name: 'Retry' }).click();

    await expect(page.getByRole('alert').filter({ hasText: 'query engine unavailable' })).toHaveCount(0);
  });

  test('Home does not call failed alerts zero, nor missing schedules unconfigured', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.route('**/api/v1/alerts/incidents**', (route) => route.fulfill(FAILURE));
    await page.route('**/api/v1/schedules', (route) => route.fulfill(FAILURE));

    await page.goto('/home');

    await expect(page.getByText('Could not be fetched').first()).toBeVisible();
    await expect(page.getByText('Could not load the schedules')).toBeVisible();
  });
});

test.describe('a list on its way', () => {
  test('keeps its head and the place of its rows', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.route('**/api/v1/alerts/rules', () => undefined);

    await page.goto('/alerts/rules');

    const table = page.locator('[data-data-table-loading]');
    await expect(table).toBeVisible();
    await expect(table.getByRole('columnheader', { name: 'Rule' })).toBeVisible();
    await expect(table).toHaveAttribute('aria-busy', 'true');
    // Nobody knows the counts yet: they are not zero.
    await expect(page.locator('[data-alert-filter-tabs]').getByRole('button', { name: /All/ })).toHaveText(/–/);
  });
});

test.describe('a draft', () => {
  test('in a drawer is not lost to Escape', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/alerts/silences');

    await page.getByRole('button', { name: /New silence/ }).click();
    const name = page.getByRole('dialog').getByLabel('Name');
    await name.fill('half a silence');

    await page.keyboard.press('Escape');
    const prompt = page.getByTestId('unsaved-prompt');
    await expect(prompt).toBeVisible();

    await prompt.getByRole('button', { name: 'Keep editing' }).click();
    await expect(prompt).toHaveCount(0);
    await expect(name).toHaveValue('half a silence');

    await page.keyboard.press('Escape');
    await page.getByTestId('unsaved-prompt').getByRole('button', { name: 'Discard' }).click();
    await expect(page.getByRole('dialog')).toHaveCount(0);
  });

  test('on the rule editor is not lost to a link, and its own Cancel is taken at its word', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/alerts/rules/new');

    await page.getByPlaceholder('e.g. api_high_error_rate', { exact: true }).fill('half a rule');
    await page.locator('aside a[href="/home"], nav a[href="/home"]').first().click();

    const prompt = page.getByTestId('unsaved-prompt');
    await expect(prompt).toBeVisible();
    await prompt.getByRole('button', { name: 'Keep editing' }).click();
    await expect(page).toHaveURL(/\/alerts\/rules\/new$/);

    await page.getByRole('button', { name: 'Cancel', exact: true }).click();
    await expect(page).toHaveURL(/\/alerts\/rules$/);
    await expect(page.getByTestId('unsaved-prompt')).toHaveCount(0);
  });
});

test.describe('a destructive confirmation', () => {
  test('is readable in the light theme and speaks the page’s language', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { theme: 'light', preferences: { language: 'zh-cn' } });
    await page.goto('/alerts/silences');

    await page.getByRole('button', { name: /删除静默|EU database upgrade/ }).first().click();

    const dialog = page.getByRole('dialog');
    await expect(dialog.getByRole('button', { name: '取消' })).toBeVisible();
    const action = dialog.getByRole('button', { name: '删除', exact: true });
    await expect(action).toHaveCSS('color', 'rgb(255, 255, 255)');
    // This silence is holding notifications back right now, and that is said.
    await expect(dialog.getByText('删除后通知会立即恢复')).toBeVisible();
  });
});

test.describe('where one cannot go', () => {
  test('a page the role does not include says so instead of moving on', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, {
      role: 'Viewer',
      scope: 'organization',
      capabilityPermissions: ['dashboards.read'],
    });

    await page.goto('/iam/roles');

    const denied = page.getByTestId('access-denied');
    await expect(denied).toBeVisible();
    await expect(page).toHaveURL(/\/iam\/roles(?:[?#]|$)/);
    await expect(denied.getByRole('button', { name: 'Copy an access request' })).toBeVisible();

    await denied.getByRole('link', { name: 'Go to a page you can open' }).click();
    await expect(page).not.toHaveURL(/\/iam\/roles/);
  });

  test('an address with a typo offers the page it may have meant', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);

    await page.goto('/settings/genral');

    await expect(page.getByRole('status').getByText('Page not found', { exact: true })).toBeVisible();
    await expect(page.locator('[data-not-found-suggestions] a').first()).toHaveAttribute('href', '/settings/general');
  });
});
