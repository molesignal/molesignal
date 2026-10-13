import type { Locator } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('settings navigation', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.addInitScript(() => {
      if (window.sessionStorage.getItem('settings_navigation_test_ready')) {
        return;
      }
      window.localStorage.removeItem('settings_sidebar_collapsed');
      window.sessionStorage.setItem('settings_navigation_test_ready', 'true');
    });
  });

  test('fully hides the desktop navigation and persists the choice', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/settings/general');

    await page
      .getByRole('button', { name: 'Collapse settings navigation' })
      .click();

    const content = page.locator('[data-management-content]');
    await expect(content).toHaveAttribute('data-sections-collapsed', 'true');
    await expect(
      page.getByRole('button', { name: 'Expand settings navigation' }),
    ).toBeVisible();
    await expect(page.locator('nav[aria-label="Settings"]')).toBeHidden();
    expect(
      await content.evaluate((element) =>
        getComputedStyle(element.parentElement!).gridTemplateColumns
          .trim()
          .split(/\s+/),
      ),
    ).toHaveLength(1);
    expect(
      await page.evaluate(() =>
        window.localStorage.getItem('settings_sidebar_collapsed'),
      ),
    ).toBe('true');

    await page.reload();
    await expect(
      page.getByRole('button', { name: 'Expand settings navigation' }),
    ).toBeVisible();

    await page
      .getByRole('button', { name: 'Expand settings navigation' })
      .click();
    await expect(page.locator('nav[aria-label="Settings"]')).toBeVisible();
    expect(
      await page.evaluate(() =>
        window.localStorage.getItem('settings_sidebar_collapsed'),
      ),
    ).toBe('false');
  });

  test('chunks the navigation into labelled blocks and glides to the page you open', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/settings/notify/connectors');

    const nav = page.locator('nav[aria-label="Settings"]');
    const indicator = page.getByTestId('management-nav-indicator');
    const centreY = async (locator: Locator) => {
      const box = (await locator.boundingBox())!;
      return box.y + box.height / 2;
    };

    // Computed colours come back in different colour spaces, so each one is
    // painted to compare it with the token it should be.
    const painted = (value: string) =>
      page.evaluate((colourValue) => {
        const canvas = document.createElement('canvas');
        canvas.width = 1;
        canvas.height = 1;
        const context = canvas.getContext('2d')!;
        context.fillStyle = colourValue;
        context.fillRect(0, 0, 1, 1);
        return Array.from(context.getImageData(0, 0, 1, 1).data).join();
      }, value);
    const token = (variable: string) =>
      page.evaluate((name) => {
        const probe = document.createElement('span');
        probe.style.color = `var(${name})`;
        document.body.append(probe);
        const value = getComputedStyle(probe).color;
        probe.remove();
        return value;
      }, variable);
    const labelIs = async (name: string, variable: string) =>
      (await painted(
        await nav.getByText(name, { exact: true }).evaluate((el) => getComputedStyle(el).color),
      )) === (await painted(await token(variable)));

    // The block that holds the current page names itself a tier darker.
    expect(await labelIs('Notify', '--tx-0')).toBe(true);
    expect(await labelIs('Data plane', '--tx-2')).toBe(true);
    expect(await labelIs('Data security', '--tx-2')).toBe(true);

    // A hairline closes each block but the last.
    expect(await nav.locator('[aria-hidden].h-px').count()).toBeGreaterThanOrEqual(3);

    const connectors = nav.getByRole('link', { name: 'Connectors', exact: true });
    await expect(connectors).toHaveAttribute('aria-current', 'page');
    await expect
      .poll(async () => Math.abs((await centreY(indicator)) - (await centreY(connectors))))
      .toBeLessThanOrEqual(1);

    await nav.getByRole('link', { name: 'Audit', exact: true }).click();
    await expect(page).toHaveURL(/\/settings\/audit/);
    const audit = nav.getByRole('link', { name: 'Audit', exact: true });
    await expect
      .poll(async () => Math.abs((await centreY(indicator)) - (await centreY(audit))))
      .toBeLessThanOrEqual(1);
    // The darker name moved with the page: Data security now holds it, Notify does not.
    // The names ease from one tier to the other, so poll until they have arrived.
    await expect.poll(() => labelIs('Data security', '--tx-0')).toBe(true);
    await expect.poll(() => labelIs('Notify', '--tx-2')).toBe(true);
  });

  test('puts a page description under its title, whole, on the same left edge', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto('/settings/notify/policies');

    const heading = page.getByRole('heading', { level: 2, name: 'Notify policies' });
    const description = page.getByText(/^Match events, resolve recipients/);
    await expect(description).toBeVisible();

    const headingBox = (await heading.boundingBox())!;
    const descriptionBox = (await description.boundingBox())!;
    expect(descriptionBox.y).toBeGreaterThanOrEqual(headingBox.y + headingBox.height - 1);
    expect(Math.abs(descriptionBox.x - headingBox.x)).toBeLessThanOrEqual(1);
    // It wraps rather than being cut off.
    expect(await description.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(false);
  });

  test('ends the label, the field and the section rule on one edge', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/settings/general');

    const sections = page.locator('[data-settings-section]');
    await expect(sections).toHaveCount(4);
    const section = (await sections.nth(1).boundingBox())!;
    const field = (await page.getByLabel('Organization name').boundingBox())!;

    // Label 280 + gap 32 + field 560 is exactly the form column.
    expect(Math.abs(field.x + field.width - (section.x + section.width))).toBeLessThanOrEqual(1);
    expect(section.width).toBeLessThanOrEqual(872);

    // Switches start where the fields do instead of drifting to the far edge.
    const toggle = (await page.getByRole('switch', { name: 'Allow self-service joining' }).boundingBox())!;
    expect(Math.abs(toggle.x - field.x)).toBeLessThanOrEqual(2);
  });

  test('folds the navigation into a drawer below desktop width', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 900, height: 900 });
    await page.goto('/settings/general');

    // The page stays in use on a narrow screen: the wide-screen notice belongs
    // to the NOC wallboard alone.
    await expect(
      page.getByRole('heading', { level: 2, name: 'General' }),
    ).toBeVisible();
    await expect(
      page.getByText('Molesignal is built for wide screens'),
    ).toHaveCount(0);
    await expect(page.locator('nav[aria-label="Settings"]')).toBeHidden();
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth - window.innerWidth,
      ),
    ).toBeLessThanOrEqual(0);

    await page
      .getByRole('button', { name: 'Open settings navigation' })
      .click();
    const drawer = page.getByRole('dialog', { name: 'Settings' });
    await expect(
      drawer.getByRole('navigation', { name: 'Settings' }),
    ).toBeVisible();

    // Choosing a page goes there and puts the drawer away.
    await drawer.getByRole('link', { name: 'Audit', exact: true }).click();
    await expect(page).toHaveURL(/\/settings\/audit/);
    await expect(drawer).toHaveCount(0);
  });

  test('keeps investigation routes in use below desktop width', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 900, height: 900 });
    await page.goto('/logs');

    await expect(
      page.getByRole('heading', { level: 1, name: 'Logs' }),
    ).toBeVisible();
    await expect(
      page.getByText('Molesignal is built for wide screens'),
    ).toHaveCount(0);
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth - window.innerWidth,
      ),
    ).toBeLessThanOrEqual(0);
  });
});
