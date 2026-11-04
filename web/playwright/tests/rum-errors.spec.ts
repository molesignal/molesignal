import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { issueIdOf, MOCK_RUM, mockRumData } from '../fixtures/mockRumData';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);

const WEB = MOCK_RUM.applications.web;

/** The three issues the mock errors are grouped into. */
const ISSUE = {
  typeError: issueIdOf(WEB, 'TypeError', "Cannot read properties of undefined (reading 'price')"),
  timeout: issueIdOf(WEB, 'GatewayTimeout', 'Payment provider timeout'),
  failed: issueIdOf(WEB, 'ServerError', 'Failed to fetch /api/orders: 500 Internal Server Error'),
} as const;

const row = (page: Page, id: string) => page.locator(`[data-rum-issue="${id}"]`);

/**
 * Opens the page and waits for the time range the page puts in the address, as it does 80 ms after
 * it mounts: a click made before that would be written over by it.
 */
async function open(page: Page, path = '/rum/errors') {
  await page.goto(path);
  await expect(page).toHaveURL(/[?&]time=/);
}

async function useChinese(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('molesignal-ui-prefs', JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }));
  });
}

test.describe('RUM errors', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('lists the issues the errors are grouped into, with the numbers of the rows they are made of', async ({ page }) => {
    await open(page);
    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('Errors');
    await expect(page.locator('[data-rum-scope="application"]')).toContainText('Storefront');

    // The same type and message in two sessions is one issue; its numbers are counted, not stored.
    const errors = DATA.errors.filter((error) => error['application.id'] === WEB);
    const sessionsOf = (id: string) => new Set(errors.filter((error) => error['error.issue_id'] === id).map((error) => error['session.id'])).size;

    await expect(page.locator('[data-rum-issue]')).toHaveCount(3);
    await expect(page.locator('[data-rum-issue-count]')).toHaveText('3 issues');

    for (const id of Object.values(ISSUE)) {
      expect(sessionsOf(id)).toBeGreaterThan(0);
      await expect(row(page, id).getByRole('cell').nth(3)).toHaveText(String(sessionsOf(id)));
    }

    // What the rules and the people made of each.
    await expect(row(page, ISSUE.typeError)).toHaveAttribute('data-rum-issue-state', 'open');
    await expect(row(page, ISSUE.timeout)).toHaveAttribute('data-rum-issue-state', 'investigating');
    await expect(row(page, ISSUE.failed)).toHaveAttribute('data-rum-issue-state', 'regressed');
    await expect(row(page, ISSUE.timeout)).toContainText('Amara Okafor');
    await expect(row(page, ISSUE.failed)).toContainText('2.3.9 → 2.4.0');
    await expect(row(page, ISSUE.typeError)).toContainText('src/checkout/Totals.tsx:88 · renderTotals');
  });

  test('says where the issues happen, and a value picked there narrows the list and what is beside it', async ({ page }) => {
    await open(page);

    const versions = page.locator('[data-rum-spread="version"] [data-rum-bar]');
    await expect(versions).toHaveCount(2);

    await page.locator('[data-rum-spread="version"]').getByRole('button', { name: '2.4.0' }).click();
    await expect(page).toHaveURL(/[?&]version=2\.4\.0/);
    await expect(page.locator('[data-rum-issue]')).toHaveCount(1);
    await expect(row(page, ISSUE.failed)).toBeVisible();
    await expect(page.locator('[data-rum-picked="version"]')).toContainText('2.4.0');

    // What is picked narrows every column; it stays in its own to be let go of.
    await expect(page.locator('[data-rum-spread="version"] [data-rum-bar]')).toHaveCount(1);
    await expect(page.locator('[data-rum-spread="version"]').getByRole('button', { name: '2.4.0' })).toHaveAttribute('aria-pressed', 'true');

    await page.locator('[data-rum-picked="version"]').click();
    await expect(page).not.toHaveURL(/version=/);
    await expect(page.locator('[data-rum-issue]')).toHaveCount(3);
  });

  test('narrows by words, by state and by who has it', async ({ page }) => {
    await open(page);

    await page.getByPlaceholder('Search issues: message, type, file').fill('timeout');
    await expect(page).toHaveURL(/[?&]q=timeout/);
    await expect(page.locator('[data-rum-issue]')).toHaveCount(1);
    await expect(row(page, ISSUE.timeout)).toBeVisible();

    await page.getByPlaceholder('Search issues: message, type, file').fill('');
    await expect(page.locator('[data-rum-issue]')).toHaveCount(3);

    await page.locator('[data-rum-filter="holder"]').click();
    await page.getByRole('option', { name: 'Unassigned' }).click();
    await expect(page).toHaveURL(/[?&]assignee=none/);
    await expect(page.locator('[data-rum-issue]')).toHaveCount(1);
    await expect(row(page, ISSUE.typeError)).toBeVisible();

    await page.locator('[data-rum-filter="status"]').click();
    await page.getByRole('option', { name: 'Resolved', exact: true }).click();
    await expect(page).toHaveURL(/[?&]status=resolved/);
    await expect(page.getByText('No issues in this range')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Show all issues' })).toBeVisible();
  });

  test('opens an issue from its row, with the address saying which, and lets go of it on Escape', async ({ page }) => {
    await open(page);
    await row(page, ISSUE.typeError).locator('button').first().click();

    await expect(page).toHaveURL(new RegExp(`[?&]issue=${ISSUE.typeError}`));

    const drawer = page.locator('[data-rum-issue-drawer]');
    await expect(drawer.locator('[data-rum-issue-title]')).toHaveText('TypeError');
    await expect(drawer.locator('[data-rum-issue-facts]')).toContainText('Open');
    await expect(drawer.locator('[data-rum-issue-message]')).toContainText("Cannot read properties of undefined (reading 'price')");

    // The stack, as a source map restored it: the application's own frame in strong type.
    await expect(drawer.locator('[data-rum-frame="app"]')).toContainText('renderTotals');
    await expect(drawer.locator('[data-rum-frame="app"]')).toContainText('src/checkout/Totals.tsx:88:14');
    await expect(drawer.locator('[data-rum-frame="library"]')).toHaveCount(1);

    await drawer.getByRole('radio', { name: 'Original' }).click();
    await expect(drawer.locator('[data-rum-frame="app"]')).toContainText('app.4f3a.js:1:20431');

    // The sessions it met, each with the way into it.
    // Two errors in one session are one session.
    await expect(drawer.locator('[data-rum-sample]')).toHaveCount(1);
    await expect(drawer.locator('[data-rum-sample]')).toContainText('u-3307');
    await expect(drawer.locator('[data-rum-people]')).toContainText('u-3307');

    await expect(drawer.locator('[data-rum-issue-explore]')).toHaveAttribute(
      'href',
      new RegExp(`/rum/explorer\\?type=errors&q=.*error\\.issue_id`),
    );

    await page.keyboard.press('Escape');
    await expect(page).not.toHaveURL(/issue=/);
    await expect(drawer).toHaveCount(0);
  });

  test('says why an issue that was resolved is open again', async ({ page }) => {
    await page.goto(`/rum/errors?issue=${ISSUE.failed}`);

    const banner = page.locator('[data-rum-issue-banner]');
    await expect(banner).toContainText('Resolved by Dana Reyes');
    await expect(banner).toContainText('in 2.3.9');
    await expect(banner).toContainText('came back in 2.4.0');
    await expect(banner).toContainText('Errors in 2.3.9 or older versions do not reopen it');

    // Where it was resolved is marked on the days it is drawn over.
    await expect(page.locator('[data-rum-trend-resolved]')).toContainText('2.3.9');

    // The rules set this state; a person cannot choose it.
    await page.locator('[data-rum-control="status"]').click();
    await expect(page.getByRole('option', { name: 'Regressed' })).toHaveAttribute('aria-disabled', 'true');
  });

  test('sets the state and the holder of an issue, and the list is of what is set', async ({ page }) => {
    await open(page, `/rum/errors?issue=${ISSUE.typeError}`);

    const drawer = page.locator('[data-rum-issue-drawer]');

    await drawer.locator('[data-rum-control="holder"]').click();
    await page.getByRole('option', { name: 'Dana Reyes' }).click();
    await expect(page.getByText('Issue updated').first()).toBeVisible();

    await drawer.locator('[data-rum-control="status"]').click();
    await page.getByRole('option', { name: 'Resolved', exact: true }).click();
    await expect(drawer.locator('[data-rum-issue-facts]')).toContainText('Resolved');

    await drawer.getByRole('button', { name: 'Close' }).click();
    await expect(drawer).toHaveCount(0);

    // Resolved issues are a choice away.
    await expect(page.locator('[data-rum-issue]')).toHaveCount(2);
    await expect(row(page, ISSUE.typeError)).toHaveCount(0);

    await page.locator('[data-rum-filter="status"]').click();
    await page.getByRole('option', { name: 'Resolved', exact: true }).click();
    await expect(page.locator('[data-rum-issue]')).toHaveCount(1);
    await expect(row(page, ISSUE.typeError)).toHaveAttribute('data-rum-issue-state', 'resolved');
    await expect(row(page, ISSUE.typeError)).toContainText('Dana Reyes');
  });

  test('is only a reading for someone who may not manage issues', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { role: 'viewer', capabilityPermissions: ['streams.query', 'rum.issues.read', 'rum.events.read'] });
    await page.goto(`/rum/errors?issue=${ISSUE.typeError}`);

    const drawer = page.locator('[data-rum-issue-drawer]');
    await expect(drawer.locator('[data-rum-issue-facts]')).toContainText('Open');
    await expect(drawer.locator('[data-rum-control="status"]')).toBeDisabled();
    await expect(drawer.locator('[data-rum-control="holder"]')).toBeDisabled();
  });

  test('is told about on the overview, where the new and the regressed issues lead to the errors page', async ({ page }) => {
    await open(page, '/rum/overview');

    const latest = page.locator('[data-rum-overview-card="issues"]');
    await expect(latest.locator('[data-rum-latest-issue]')).toHaveCount(3);
    await expect(latest.locator(`[data-rum-latest-issue="${ISSUE.failed}"]`)).toContainText('Resolved in 2.3.9, back in 2.4.0');
    await expect(latest.locator(`[data-rum-latest-issue="${ISSUE.typeError}"]`)).toContainText('First seen');

    await latest.locator(`[data-rum-latest-issue="${ISSUE.timeout}"] a`).click();
    await expect(page).toHaveURL(new RegExp(`/rum/errors\\?.*issue=${ISSUE.timeout}`));
    await expect(page.locator('[data-rum-issue-drawer] [data-rum-issue-title]')).toHaveText('GatewayTimeout');
  });

  test('says how to start before the first application exists', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/applications(?:\?.*)?$/, (route) => route.fulfill({ json: [] }));
    await page.goto('/rum/errors');

    await expect(page.getByTestId('rum-activation').getByRole('heading', { level: 2 })).toHaveText('Start collecting real user experience data');
  });

  test('says why there are no numbers when counting took too long', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/issues\?.*/, (route) => route.fulfill({ json: { issues: [], total: 0, unavailable: 'time_budget' } }));
    await page.goto('/rum/errors');

    await expect(page.getByText('Counting took longer than the time budget')).toBeVisible();
  });
});

test.describe('RUM errors in Chinese', () => {
  test('say the states and the columns in the language of the product', async ({ page, mockServer }) => {
    await useChinese(page);
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await page.goto('/rum/errors');

    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('错误');
    await expect(page.locator('[data-rum-issue-count]')).toHaveText('3 个 issue');
    await expect(row(page, ISSUE.typeError)).toContainText('待处理');
    await expect(row(page, ISSUE.timeout)).toContainText('调查中');
    await expect(row(page, ISSUE.failed)).toContainText('回归');
    await expect(page.getByRole('columnheader', { name: '最近出现' })).toBeVisible();
  });
});
