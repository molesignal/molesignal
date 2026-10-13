import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const TABS = [
  ['Insights', '/alerts/insights'],
  ['Incidents', '/alerts/incidents'],
  ['Rules', '/alerts/rules'],
  ['History', '/alerts/history'],
  ['Silence policies', '/alerts/silences'],
  ['Escalations', '/alerts/escalations'],
  ['On-call schedules', '/alerts/schedules'],
  ['Groups', '/alerts/semantic-groups'],
] as const;

test.describe('alerts', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('keeps one title over every tab of the module', async ({ page }) => {
    await page.goto('/alerts');
    await expect(page).toHaveURL(/\/alerts\/incidents$/);

    const tabs = page.getByTestId('alerts-subnav');
    await expect(tabs.getByRole('link')).toHaveCount(TABS.length);

    for (const [name, path] of TABS) {
      await tabs.getByRole('link', { name, exact: true }).click();
      await expect(page).toHaveURL(new RegExp(`${path}(\\?.*)?$`));
      // The module has one name; the tab that is open is said by the tabs.
      await expect(page.getByRole('heading', { level: 1 })).toHaveText('Alerts');
      await expect(tabs.getByRole('link', { name, exact: true })).toHaveAttribute(
        'aria-current',
        'page',
      );
    }
  });

  test('opens on what is firing, the worst first, with who has it', async ({ page }) => {
    await page.goto('/alerts/incidents');

    const standing = page.getByRole('region', { name: 'Incidents now' });
    await expect(standing.getByRole('heading')).toHaveText('3 incidents');
    await expect(standing).toContainText('api-gateway, checkout, web-dashboard');
    // One statement, not a row of number cards.
    await expect(page.locator('[data-statement]')).toHaveCount(1);

    const rows = page.locator('tbody tr');
    await expect(rows).toHaveCount(3);
    await expect(rows.first()).toContainText('Critical');

    // The statement has counted what waits; the view only chooses it.
    await page
      .locator('[data-alert-filter-tabs]')
      .getByRole('button', { name: 'Waiting' })
      .click();
    await expect(rows).toHaveCount(2);
    await expect(rows.first().getByRole('button', { name: 'Acknowledge' })).toBeVisible();
  });

  test('lists the rules with their views at the head of the list', async ({ page }) => {
    await page.goto('/alerts/rules');

    const head = page.locator('[data-list-surface] [data-list-head]');
    await expect(head.getByPlaceholder('Search rules, services, or sources')).toBeVisible();
    const views = head.locator('[data-alert-filter-tabs]');
    await expect(views.getByRole('button', { name: /^All/ })).toContainText('8');
    await expect(page.locator('tbody tr')).toHaveCount(8);

    await views.getByRole('button', { name: /^Disabled/ }).click();
    await expect(page.locator('tbody tr')).toHaveCount(1);
    await expect(page.getByTestId('page-header').getByRole('button')).toHaveCount(1);
  });

  test('says how each silence and each escalation stands', async ({ page }) => {
    await page.goto('/alerts/silences');
    await expect(page.locator('tbody tr').first()).toContainText('Silencing now');

    await page.goto('/alerts/escalations');
    const policy = page.locator('tbody tr').first();
    await expect(policy).toContainText('Platform on-call');
    await expect(policy).toContainText('from Error');
    await expect(policy).toContainText('repeats 3 times');
  });

  test('opens an incident beside the list, and on a page of its own', async ({ page }) => {
    await page.goto('/alerts/incidents');
    await page.locator('tbody tr').first().click();
    await expect(page.getByRole('dialog')).toContainText(
      'Public API answers 16% of requests with 502',
    );

    // The page is the address an incident is shared by; it opens with how the incident stands.
    await page.goto('/alerts/incidents/inc-2291');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText(
      'Public API answers 16% of requests with 502',
    );
    await expect(page.locator('[data-statement]').getByRole('heading')).toHaveText('Firing for 38m');
    await expect(page.getByTestId('incident-resolve')).toBeVisible();
    // The module's tabs belong to the module's pages.
    await expect(page.getByTestId('alerts-subnav')).toHaveCount(0);
  });
});
