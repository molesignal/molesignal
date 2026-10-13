import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const TABS = [
  ['Overview', '/synthetics/overview'],
  ['Checks', '/synthetics/checks'],
  ['Results', '/synthetics/results'],
  ['Locations', '/synthetics/locations'],
  ['Agents', '/synthetics/agents'],
  ['Variables', '/synthetics/variables'],
] as const;

test.describe('synthetics', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('keeps one title over six tabs, each with at most one action', async ({ page }) => {
    await page.goto('/synthetics');
    await expect(page).toHaveURL(/\/synthetics\/overview$/);

    const tabs = page.getByTestId('synthetics-subnav');
    await expect(tabs.getByRole('link')).toHaveCount(TABS.length);

    for (const [name, path] of TABS) {
      await tabs.getByRole('link', { name, exact: true }).click();
      await expect(page).toHaveURL(new RegExp(`${path}$`));
      // The module has one name; the tab that is open is said by the tabs.
      await expect(page.getByRole('heading', { level: 1 })).toHaveText('Synthetics');
      await expect(tabs.getByRole('link', { name, exact: true })).toHaveAttribute(
        'aria-current',
        'page',
      );
      // The data follows the checks on its own: nothing asks to be refreshed by hand.
      await expect(page.getByRole('button', { name: 'Refresh' })).toHaveCount(0);
      const filled = await page
        .getByTestId('page-header')
        .locator('button')
        .evaluateAll((buttons) => buttons.length);
      expect(filled, `${name} offers more than one action`).toBeLessThanOrEqual(1);
    }
  });

  test('opens with what the checks say, then the checks that need attention', async ({ page }) => {
    await page.goto('/synthetics/overview');

    const standing = page.getByRole('region', { name: 'Synthetic checks now' });
    await expect(standing.getByRole('heading')).toHaveText('1 check is failing');
    await expect(standing).toContainText('10 checks running · 1 paused');
    // One statement, not a row of number cards.
    await expect(page.locator('[data-statement]')).toHaveCount(1);

    const attention = page.getByRole('region', { name: 'Needs attention' });
    await expect(attention.getByRole('link')).toHaveCount(3);
    // The worst first.
    await expect(attention.getByRole('link').first()).toContainText('Sign-in journey');

    await attention.getByRole('link', { name: 'Open Sign-in journey' }).click();
    await expect(page).toHaveURL(/\/synthetics\/checks\/mon-login-journey$/);
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('Sign-in journey');
  });

  test('gives a check a page of its own, with its own tabs', async ({ page }) => {
    await page.goto('/synthetics/checks/mon-login-journey');

    // The module's tabs belong to the module's pages.
    await expect(page.getByTestId('synthetics-subnav')).toHaveCount(0);
    const tabs = page.getByTestId('synthetics-check-tabs');
    await expect(tabs.getByRole('link')).toHaveCount(4);

    const standing = page.getByRole('region', { name: 'The check now' });
    await expect(standing.getByRole('heading')).toHaveText('Failing');
    // The check runs from two places; each is one row, with how its runs went there.
    await expect(
      page.getByRole('region', { name: 'By location', exact: true }).locator('[data-check-location]'),
    ).toHaveCount(2);
    await expect(page.getByRole('button', { name: 'Run now' })).toBeVisible();

    await tabs.getByRole('link', { name: 'Results' }).click();
    await expect(page).toHaveURL(/\/synthetics\/checks\/mon-login-journey\/results$/);
    await page.getByRole('row').nth(1).click();
    await expect(page).toHaveURL(/\/results\/[^/]+$/);
    await expect(page.getByRole('dialog')).toBeVisible();

    await page.keyboard.press('Escape');
    await tabs.getByRole('link', { name: 'Configuration' }).click();
    await expect(page.getByRole('region', { name: 'Configuration' })).toContainText('Location policy');
  });

  test('offers the kinds of check as views of one list', async ({ page }) => {
    await page.goto('/synthetics/checks');

    const views = page.getByRole('group', { name: 'Kinds of check' });
    await expect(views.getByRole('button', { name: /^All/ })).toContainText('11');
    await expect(page.getByRole('row')).toHaveCount(12);
    // Each row draws its latest runs.
    await expect(page.locator('[data-run-strip]').first()).toBeVisible();

    await views.getByRole('button', { name: /^Browser/ }).click();
    await expect(page).toHaveURL(/\/synthetics\/checks\?kind=browser$/);
    await expect(page.getByRole('row')).toHaveCount(2);
    await expect(page.getByRole('row').nth(1)).toContainText('Sign-in journey');
  });

  // One test for each address: a page load apiece, so no address waits on the
  // time the others took.
  for (const [gone, now] of [
    ['/synthetics/browser-tests', /\/synthetics\/checks\?kind=browser$/],
    ['/synthetics/network', /\/synthetics\/checks\?kind=network$/],
    ['/synthetics/schedules', /\/synthetics\/checks$/],
    ['/synthetics/settings', /\/synthetics\/overview$/],
    ['/synthetics/assertions', /\/synthetics\/checks\/new$/],
  ] as const) {
    test(`sends ${gone}, the address of a tab that is gone, to where its content is`, async ({
      page,
    }) => {
      await page.goto(gone);
      await expect(page).toHaveURL(now);

      if (gone === '/synthetics/browser-tests') {
        await expect(page.getByRole('row')).toHaveCount(2);
      }
      if (gone === '/synthetics/assertions') {
        await expect(page.getByRole('dialog')).toBeVisible();
      }
    });
  }

  test('says whether a location is online, not whether it passed', async ({ page }) => {
    await page.goto('/synthetics/locations');

    const rows = page.getByRole('row');
    await expect(rows).toHaveCount(5);
    await expect(rows.nth(1)).toContainText('Online');
    await expect(rows.nth(3)).toContainText('Degraded');
    await expect(page.getByText('Passed', { exact: true })).toHaveCount(0);
  });

  test('says how to begin when there is no check', async ({ page }) => {
    await page.route(/\/api\/v1\/synthetics\/monitors(?:\?.*)?$/, (route) =>
      route.fulfill({ json: [] }),
    );
    await page.goto('/synthetics/overview');

    const firstRun = page.getByTestId('synthetics-first-run');
    await expect(firstRun.getByRole('heading', { level: 2 })).toHaveText(
      'Check your services from the outside',
    );
    // The first-run page carries the one way in; the header does not repeat it.
    await expect(page.getByRole('button', { name: 'Create check' })).toHaveCount(1);
    await firstRun.getByRole('link', { name: 'Certificate validity' }).click();
    await expect(page).toHaveURL(/\/synthetics\/checks\/new\?preset=tls$/);
  });
});
