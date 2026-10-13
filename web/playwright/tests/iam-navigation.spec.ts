import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('IAM navigation', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.addInitScript(() => {
      if (window.sessionStorage.getItem('iam_navigation_test_ready')) return;
      window.localStorage.removeItem('iam_sidebar_collapsed');
      window.sessionStorage.setItem('iam_navigation_test_ready', 'true');
    });
  });

  test('fully hides the desktop navigation and persists the choice', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/iam/roles');

    await page
      .getByRole('button', {
        name: 'Collapse identity and access navigation',
      })
      .click();

    const content = page.locator('[data-management-content]');
    await expect(content).toHaveAttribute('data-sections-collapsed', 'true');
    await expect(
      page.getByRole('button', {
        name: 'Expand identity and access navigation',
      }),
    ).toBeVisible();
    await expect(
      page.locator('nav[aria-label="Identity & Access"]'),
    ).toBeHidden();
    expect(
      await content.evaluate((element) =>
        getComputedStyle(element.parentElement!).gridTemplateColumns
          .trim()
          .split(/\s+/),
      ),
    ).toHaveLength(1);
    expect(
      await page.evaluate(() =>
        window.localStorage.getItem('iam_sidebar_collapsed'),
      ),
    ).toBe('true');

    await page.reload();
    await expect(
      page.getByRole('button', {
        name: 'Expand identity and access navigation',
      }),
    ).toBeVisible();

    await page
      .getByRole('button', {
        name: 'Expand identity and access navigation',
      })
      .click();
    await expect(
      page.locator('nav[aria-label="Identity & Access"]'),
    ).toBeVisible();
    expect(
      await page.evaluate(() =>
        window.localStorage.getItem('iam_sidebar_collapsed'),
      ),
    ).toBe('false');
  });

  test('folds the navigation into a drawer below desktop width', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 900, height: 900 });
    await page.goto('/iam/roles');

    // The page stays in use on a narrow screen: the wide-screen notice belongs
    // to the NOC wallboard alone.
    await expect(
      page.getByRole('heading', { level: 2, name: 'Roles' }),
    ).toBeVisible();
    await expect(
      page.getByText('Molesignal is built for wide screens'),
    ).toHaveCount(0);
    await expect(
      page.locator('nav[aria-label="Identity & Access"]'),
    ).toBeHidden();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth - window.innerWidth,
      ),
    ).toBeLessThanOrEqual(0);

    await page
      .getByRole('button', { name: 'Open identity and access navigation' })
      .click();
    const drawer = page.getByRole('dialog', { name: 'Identity & Access' });
    await expect(
      drawer.getByRole('navigation', { name: 'Identity & Access' }),
    ).toBeVisible();

    // Choosing a page goes there and puts the drawer away.
    await drawer.getByRole('link', { name: 'Users', exact: true }).click();
    await expect(page).toHaveURL(/\/iam\/users/);
    await expect(drawer).toHaveCount(0);
  });
});
