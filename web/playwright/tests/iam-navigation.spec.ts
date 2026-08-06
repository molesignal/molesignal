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

  test('opens IAM navigation as a drawer below desktop width', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 900, height: 900 });
    await page.goto('/iam/roles');
    await expect(page.getByText('Full administrative access.')).toBeVisible();

    const openButton = page.getByRole('button', {
      name: 'Open identity and access navigation',
    });
    await expect(openButton).toBeVisible();
    await expect(
      page.locator('nav[aria-label="Identity & Access"]'),
    ).toBeHidden();

    await openButton.click();

    const drawer = page.getByRole('dialog');
    await expect(drawer).toBeVisible();
    await expect(
      drawer.getByRole('navigation', { name: 'Identity & Access' }),
    ).toBeVisible();
    await drawer.getByRole('link', { name: 'Roles' }).click();
    await expect(drawer).toBeHidden();
  });
});
