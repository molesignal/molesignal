import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('account settings center', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/account/settings/profile');
  });

  test('keeps navigation controls clear of active items and exposes all sections', async ({
    page,
  }) => {
    const activeProfile = page.getByRole('link', {
      name: 'Profile',
      exact: true,
    });
    const collapse = page.getByRole('button', { name: 'Collapse menu' });
    const [activeBox, collapseBox] = await Promise.all([
      activeProfile.boundingBox(),
      collapse.boundingBox(),
    ]);
    expect(activeBox).not.toBeNull();
    expect(collapseBox).not.toBeNull();
    expect(
      Math.max(activeBox!.x, collapseBox!.x) <
        Math.min(
          activeBox!.x + activeBox!.width,
          collapseBox!.x + collapseBox!.width,
        ) &&
        Math.max(activeBox!.y, collapseBox!.y) <
          Math.min(
            activeBox!.y + activeBox!.height,
            collapseBox!.y + collapseBox!.height,
          ),
    ).toBe(false);

    const sections = [
      ['Preferences', 'Preferences'],
      ['Notify', 'My notify settings'],
      ['Security & sign-in', 'Security & sign-in'],
      ['Active sessions', 'Active sessions'],
      ['Workspace & identity', 'Workspace & identity'],
    ] as const;
    for (const [link, heading] of sections) {
      await page.getByRole('link', { name: link, exact: true }).click();
      await expect(
        page.getByRole('heading', { name: heading, level: 2 }),
      ).toBeVisible();
    }
  });

  test('saves profile fields through the personal profile endpoint', async ({
    page,
  }) => {
    const emailInput = page.getByRole('textbox', { name: 'Sign-in email' });
    const displayNameInput = page.getByRole('textbox', {
      name: 'Display name',
    });
    // The sign-in email is shown to be read and copied, not edited: a
    // read-only field, set apart from the ones that take input.
    await expect(emailInput).toHaveAttribute('readonly', '');
    await expect(emailInput).not.toBeEditable();
    await expect(emailInput).toHaveCSS('cursor', 'default');
    const [emailBackground, displayNameBackground] = await Promise.all([
      emailInput.evaluate((element) => getComputedStyle(element).backgroundColor),
      displayNameInput.evaluate(
        (element) => getComputedStyle(element).backgroundColor,
      ),
    ]);
    expect(emailBackground).not.toBe(displayNameBackground);

    await displayNameInput.fill('Root SRE');
    await page
      .getByRole('textbox', { name: 'About you (optional)' })
      .fill('Platform operations lead');
    const [bioBox, saveButtonBox] = await Promise.all([
      page
        .getByRole('textbox', { name: 'About you (optional)' })
        .boundingBox(),
      page.getByRole('button', { name: 'Save changes' }).boundingBox(),
    ]);
    expect(saveButtonBox!.y).toBeGreaterThan(bioBox!.y + bioBox!.height);

    const responsePromise = page.waitForResponse(
      (response) =>
        response.url().includes('/api/v1/me/profile') &&
        response.request().method() === 'PUT',
    );
    await page.getByRole('button', { name: 'Save changes' }).click();
    const response = await responsePromise;
    expect(response.request().postDataJSON()).toEqual({
      displayName: 'Root SRE',
      bio: 'Platform operations lead',
    });
    await expect(
      page.getByRole('textbox', { name: 'Display name' }),
    ).toHaveValue('Root SRE');
  });

  test('does not draw decorative dividers around account content groups', async ({
    page,
  }) => {
    const profileHeader = page
      .getByRole('heading', { name: 'Profile', level: 2 })
      .locator('xpath=ancestor::header[1]');
    await expect(profileHeader).toHaveCSS('border-bottom-width', '0px');

    await page.getByRole('link', { name: 'Preferences' }).click();
    const preferenceSections = page.locator('main').getByRole('heading', {
      level: 3,
    });
    for (let index = 0; index < (await preferenceSections.count()); index += 1) {
      const section = preferenceSections
        .nth(index)
        .locator('xpath=ancestor::section[1]');
      await expect(section).toHaveCSS('border-top-width', '0px');
    }

    await page.getByRole('link', { name: 'Notify', exact: true }).click();
    for (const name of ['Notify identities', 'Delivery preferences']) {
      const section = page
        .getByRole('heading', { name, level: 3 })
        .locator('xpath=ancestor::section[1]');
      await expect(section).toHaveCSS('border-top-width', '0px');
    }
  });
});


test('workspace identifiers have bounded, visible copy wells', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port, { theme: 'light' });
  await page.goto('/account/settings/workspace');
  await page.locator('summary').click();
  const rows = page.locator('details [data-settings-row]');
  await expect(rows).toHaveCount(3);
  for (const row of await rows.all()) {
    const well = row.locator('code').locator('..');
    const appearance = await well.evaluate((element) => ({
      background: getComputedStyle(element).backgroundColor,
      canvas: getComputedStyle(document.documentElement).getPropertyValue('--page-canvas').trim(),
      width: element.getBoundingClientRect().width,
    }));
    expect(appearance.background).not.toBe('rgba(0, 0, 0, 0)');
    expect(appearance.width).toBeLessThanOrEqual(560);
    const differsFromCanvas = await well.evaluate((element) => {
      const probe = document.createElement('div');
      probe.style.backgroundColor = 'var(--page-canvas)';
      document.body.appendChild(probe);
      const different = getComputedStyle(probe).backgroundColor !== getComputedStyle(element).backgroundColor;
      probe.remove();
      return different;
    });
    expect(differsFromCanvas).toBe(true);
    await expect(well.getByRole('button')).toHaveCount(1);
  }
  await page.screenshot({ path: '/tmp/molesignal-workspace-identity.png' });
});
