import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const SHARE = '**/api/v1/public/share';
const UNLOCK = '**/api/v1/public/share/unlock';
const UNAVAILABLE_HINT =
  'This link is no longer valid, or the shared content has been deleted. Ask the owner to create a new link.';

function refuseFor(seconds: number) {
  return {
    status: 429,
    headers: { 'retry-after': String(seconds) },
    json: {
      error: 'resource_exhausted',
      code: 'signin_limited',
      message: `too many failed password attempts; try again in ${seconds} seconds`,
    },
  };
}

/** A new public dashboard link with a password. */
async function createPublicLink(page: Page) {
  await page.goto('/dashboards/d1');

  return page.evaluate(async () => {
    const response = await fetch('/api/v1/resource_shares', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        resourceType: 'dashboard',
        resourceId: 'd1',
        shareMode: 'public_link',
        password: 'dashboard-secret',
      }),
    });
    const created = (await response.json()) as {
      share: { id: string };
      url: string;
    };

    return { id: created.share.id, url: created.url };
  });
}

/** Lands on the password gate of a new public dashboard link. */
async function openPasswordGate(page: Page) {
  const { url } = await createPublicLink(page);

  await page.goto(url);
  await expect(page).toHaveURL(/\/shared$/);
}

/** Where a share link that cannot open lands. */
async function expectUnavailablePage(page: Page) {
  await expect(page).toHaveURL(/\/shared\?unavailable=1$/);
  await expect(
    page.getByRole('heading', { name: 'Link unavailable' }),
  ).toBeVisible();
  await expect(page.getByText(UNAVAILABLE_HINT)).toBeVisible();
}

test.describe('dashboard and report resource sharing', () => {
  test.beforeEach(async ({ page, context, mockServer }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await mountMockRoutes(page, mockServer.port);
  });

  test('dashboard authenticated share keeps IAM in the access path', async ({
    page,
  }) => {
    await page.goto('/dashboards/d1');

    const share = page.getByRole('button', { name: 'Share', exact: true });
    await expect(share).toBeVisible({ timeout: 10_000 });
    await share.click();
    const dialog = page.getByRole('dialog');
    await expect(
      dialog.getByRole('heading', { name: 'Share resource' }),
    ).toBeVisible();
    await dialog
      .getByRole('button', { name: 'Create share link' })
      .click();
    await expect(dialog.getByText('Share link is ready')).toBeVisible();
    await dialog.getByRole('button', { name: 'Copy link' }).click();

    const copied = await page.evaluate(() => navigator.clipboard.readText());
    expect(copied).toMatch(/\/s\/ms0000000001$/);

    await dialog
      .getByRole('button', { name: 'Close', exact: true })
      .first()
      .click();
    await share.click();
    const reopenedDialog = page.getByRole('dialog');
    await expect(reopenedDialog.getByTitle(copied)).toBeVisible();
    await reopenedDialog
      .getByRole('button', { name: 'Copy existing share link' })
      .click();
    await expect
      .poll(() => page.evaluate(() => navigator.clipboard.readText()))
      .toBe(copied);
    const actionCenters = await Promise.all(
      [
        reopenedDialog.getByTitle(copied),
        reopenedDialog.getByRole('button', {
          name: 'Copy existing share link',
        }),
        reopenedDialog.getByRole('button', {
          name: 'Rotate',
          exact: true,
        }),
        reopenedDialog.getByRole('button', {
          name: 'Disable',
          exact: true,
        }),
      ].map((button) =>
        button.evaluate((element) => {
          const rect = element.getBoundingClientRect();
          return rect.top + rect.height / 2;
        }),
      ),
    );
    expect(
      Math.max(...actionCenters) - Math.min(...actionCenters),
    ).toBeLessThanOrEqual(1);

    const rows = await page.evaluate(async () => {
      const response = await fetch(
        '/api/v1/resource_shares?resourceType=dashboard&resourceId=d1',
      );
      return response.json() as Promise<
        Array<{
          resourceType: string;
          resourceId: string;
          shareMode: string;
        }>
      >;
    });
    expect(rows[0]).toMatchObject({
      resourceType: 'dashboard',
      resourceId: 'd1',
      shareMode: 'authenticated',
    });

    await page.goto(copied);
    await expect(page).toHaveURL(/\/dashboards\/d1$/);
    await expect(
      page.getByRole('button', { name: 'Share', exact: true }),
    ).toBeVisible();

    await page
      .getByRole('button', { name: 'Share', exact: true })
      .click();
    const revokeDialog = page.getByRole('dialog');
    await revokeDialog
      .getByRole('button', { name: 'Disable', exact: true })
      .click();
    await expect(revokeDialog.getByTitle(copied)).toHaveCount(0);
    await expect(
      revokeDialog.getByRole('button', {
        name: 'Copy existing share link',
      }),
    ).toHaveCount(0);
    const disabledShare = await page.evaluate(async () => {
      const response = await fetch(
        '/api/v1/resource_shares?resourceType=dashboard&resourceId=d1',
      );
      return (await response.json()) as Array<{ url: string | null }>;
    });
    expect(disabledShare[0]?.url).toBeNull();
  });

  test('report authenticated share links back to the selected report', async ({
    page,
  }) => {
    await page.goto('/reports');
    await expect(page.getByText('Weekly SLO', { exact: true })).toBeVisible({
      timeout: 10_000,
    });

    await page.getByRole('button', { name: 'Share', exact: true }).click();
    const shareDialog = page.getByRole('dialog');
    await shareDialog
      .getByRole('button', { name: 'Create share link' })
      .click();
    await shareDialog.getByRole('button', { name: 'Copy link' }).click();

    const copied = await page.evaluate(() => navigator.clipboard.readText());
    expect(copied).toMatch(/\/s\/ms0000000001$/);

    await page.goto(copied);
    await expect(page).toHaveURL(/\/reports\?report=r1$/);
    const dialog = page.getByRole('dialog');
    await expect(dialog.getByRole('heading', { name: 'Export now' })).toBeVisible();
    await expect(dialog.getByRole('combobox')).toContainText('Weekly SLO');
  });

  test('public report explains and focuses a missing required password', async ({
    page,
  }) => {
    await page.goto('/reports');
    await page.evaluate(async () => {
      const response = await fetch('/api/v1/resource_shares/policy');
      const policy = (await response.json()) as Record<string, unknown>;
      await fetch('/api/v1/resource_shares/policy', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          ...policy,
          requirePublicReportPassword: true,
        }),
      });
    });

    await page.getByRole('button', { name: 'Share', exact: true }).click();
    const dialog = page.getByRole('dialog');
    await dialog
      .getByRole('radio', { name: /Anyone with the link/ })
      .click();
    await dialog
      .getByRole('button', { name: 'Create share link' })
      .click();

    const password = dialog.locator('input[type="password"]');
    await expect(password).toBeFocused();
    await expect(
      dialog.getByText(
        'Enter an access password for the public report.',
      ),
    ).toBeVisible();

    await password.fill('share-secret');
    await dialog
      .getByRole('button', { name: 'Create share link' })
      .click();
    await expect(dialog.getByText('Share link is ready')).toBeVisible();
  });

  test('public dashboard exchanges the opaque token for a restricted session', async ({
    page,
  }) => {
    let publicMetadataAuthorization: string | undefined;
    page.on('request', (request) => {
      if (request.url().endsWith('/api/v1/public/share')) {
        publicMetadataAuthorization = request.headers().authorization;
      }
    });

    await page.goto('/dashboards/d1');
    await page.getByRole('button', { name: 'Share', exact: true }).click();
    const dialog = page.getByRole('dialog');
    await dialog
      .getByRole('radio', { name: /Anyone with the link/ })
      .click();
    await dialog
      .getByRole('button', { name: 'Create share link' })
      .click();
    const password = dialog.locator('input[type="password"]');
    await expect(password).toBeFocused();
    await expect(
      dialog.getByText(
        'Enter an access password for the public dashboard.',
      ),
    ).toBeVisible();
    await password.fill('dashboard-secret');
    await dialog
      .getByRole('button', { name: 'Create share link' })
      .click();
    await dialog.getByRole('button', { name: 'Copy link' }).click();
    const copied = await page.evaluate(() => navigator.clipboard.readText());

    await page.goto(copied);
    await expect(page).toHaveURL(/\/shared$/);
    await expect(
      page.getByRole('heading', {
        name: 'This share is password protected',
      }),
    ).toBeVisible();
    await page.getByPlaceholder('Access password').fill('dashboard-secret');
    await page.getByRole('button', { name: 'Continue' }).click();
    await expect(page.getByText('Restricted read-only')).toBeVisible();
    await expect(
      page.getByRole('heading', { name: 'Web overview' }),
    ).toBeVisible();
    expect(publicMetadataAuthorization).toBeUndefined();
  });

  test('too many wrong share passwords say how long to wait', async ({
    page,
  }) => {
    await openPasswordGate(page);

    const password = page.getByPlaceholder('Access password');
    const proceed = page.getByRole('button', { name: 'Continue' });

    await password.fill('wrong-password');
    await proceed.click();
    await expect(page.getByRole('alert')).toHaveText(
      'Incorrect password. Try again.',
    );

    await page.route(UNLOCK, (route) => route.fulfill(refuseFor(840)));
    await password.fill('dashboard-secret');
    await proceed.click();
    await expect(page.getByRole('alert')).toHaveText(
      'Too many failed attempts. Try again in 14 minutes.',
    );

    // The password stays, so nobody has to type it again after the wait.
    await expect(password).toHaveValue('dashboard-secret');

    await page.unroute(UNLOCK);
    await proceed.click();
    await expect(
      page.getByRole('heading', { name: 'Web overview' }),
    ).toBeVisible();
  });

  test('a short wait for a share password is said in seconds', async ({
    page,
  }) => {
    await openPasswordGate(page);
    await page.route(UNLOCK, (route) => route.fulfill(refuseFor(42)));

    await page.getByPlaceholder('Access password').fill('dashboard-secret');
    await page.getByRole('button', { name: 'Continue' }).click();

    await expect(page.getByRole('alert')).toHaveText(
      'Too many failed attempts. Try again in 42 seconds.',
    );
  });

  test('the wait for a share password is said in the language of the page', async ({
    page,
  }) => {
    await page.addInitScript(() => {
      localStorage.setItem(
        'molesignal-ui-prefs',
        JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }),
      );
    });
    await openPasswordGate(page);
    await page.route(UNLOCK, (route) => route.fulfill(refuseFor(900)));

    await page.getByPlaceholder('访问密码').fill('dashboard-secret');
    await page.getByRole('button', { name: '继续访问' }).click();

    await expect(page.getByRole('alert')).toHaveText(
      '尝试次数太多，请 15 分钟后再试。',
    );
  });

  test('a visit that ended is not blamed on the password', async ({ page }) => {
    await openPasswordGate(page);
    await page.route(UNLOCK, (route) =>
      route.fulfill({
        status: 401,
        json: { error: 'unauthorized', message: 'share session expired' },
      }),
    );

    await page.getByPlaceholder('Access password').fill('dashboard-secret');
    await page.getByRole('button', { name: 'Continue' }).click();

    await expect(page.getByRole('alert')).toHaveText(
      'This visit has ended. Open the share link again.',
    );
  });

  test('a password that could not be checked is not called wrong', async ({
    page,
  }) => {
    await openPasswordGate(page);
    await page.route(UNLOCK, (route) =>
      route.fulfill({
        status: 500,
        json: { error: 'internal', message: 'internal error' },
      }),
    );

    await page.getByPlaceholder('Access password').fill('dashboard-secret');
    await page.getByRole('button', { name: 'Continue' }).click();

    await expect(page.getByRole('alert')).toHaveText(
      'The password could not be checked. Try again later.',
    );
  });

  test('a blank share password cannot be sent', async ({ page }) => {
    await openPasswordGate(page);

    const proceed = page.getByRole('button', { name: 'Continue' });

    await expect(proceed).toBeDisabled();
    await page.getByPlaceholder('Access password').fill('   ');
    await expect(proceed).toBeDisabled();
  });

  for (const { status, backendSays, pageSays } of [
    {
      status: 401,
      backendSays: 'share session expired',
      pageSays: 'This visit has ended. Open the share link again.',
    },
    {
      status: 404,
      backendSays: 'dashboard not found',
      pageSays: UNAVAILABLE_HINT,
    },
    {
      status: 500,
      backendSays: 'internal error',
      pageSays: 'This share could not be opened. Try again later.',
    },
  ]) {
    test(`a share answering ${status} is explained, not quoted`, async ({
      page,
    }) => {
      await page.route(SHARE, (route) =>
        route.fulfill({
          status,
          json: { error: 'share_failed', message: backendSays },
        }),
      );

      await page.goto('/shared');

      await expect(
        page.getByRole('heading', { name: 'Link unavailable' }),
      ).toBeVisible();
      await expect(page.getByText(pageSays)).toBeVisible();
      await expect(page.getByText(backendSays)).toHaveCount(0);
    });
  }

  test('a share link that does not exist lands on the unavailable page', async ({
    page,
  }) => {
    await page.goto('/s/no-such-link');

    await expectUnavailablePage(page);
  });

  test('a revoked share link lands on the unavailable page', async ({
    page,
  }) => {
    const { id, url } = await createPublicLink(page);

    await page.evaluate(async (shareId) => {
      await fetch(`/api/v1/resource_shares/${shareId}`, { method: 'DELETE' });
    }, id);
    await page.goto(url);

    await expectUnavailablePage(page);
  });

  test('a session left from another link does not stand in for a dead one', async ({
    page,
  }) => {
    await openPasswordGate(page);

    await page.goto('/s/no-such-link');

    await expectUnavailablePage(page);
    await expect(page.getByPlaceholder('Access password')).toHaveCount(0);
  });

  test('a share that ended is explained in the language of the page', async ({
    page,
  }) => {
    await page.addInitScript(() => {
      localStorage.setItem(
        'molesignal-ui-prefs',
        JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }),
      );
    });
    await page.route(SHARE, (route) =>
      route.fulfill({
        status: 401,
        json: { error: 'unauthorized', message: 'share session expired' },
      }),
    );

    await page.goto('/shared');

    await expect(page.getByRole('heading', { name: '链接不可用' })).toBeVisible();
    await expect(
      page.getByText('这次访问已结束，请重新打开分享链接。'),
    ).toBeVisible();
    await expect(page.getByText('share session expired')).toHaveCount(0);
  });

  test('production dashboard policy blocks only public sharing before submit', async ({
    page,
  }) => {
    await page.goto('/dashboards/d1');
    await page.evaluate(async () => {
      const [dashboardResponse, policyResponse] = await Promise.all([
        fetch('/api/v1/dashboards/d1'),
        fetch('/api/v1/resource_shares/policy'),
      ]);
      const dashboard = (await dashboardResponse.json()) as {
        model: Record<string, unknown>;
      };
      const policy = (await policyResponse.json()) as Record<
        string,
        unknown
      >;
      dashboard.model.tags = ['production'];
      await Promise.all([
        fetch('/api/v1/dashboards/d1', {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ model: dashboard.model }),
        }),
        fetch('/api/v1/resource_shares/policy', {
          method: 'PUT',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({
            ...policy,
            denyProductionPublicShares: true,
          }),
        }),
      ]);
    });
    await page.reload();

    await page.getByRole('button', { name: 'Share', exact: true }).click();
    const dialog = page.getByRole('dialog');
    await expect(
      dialog.getByRole('radio', { name: /Anyone with the link/ }),
    ).toBeDisabled();
    await expect(dialog.getByRole('alert')).toContainText(
      'workspace policy prevents production dashboards',
    );
    await expect(
      dialog.getByRole('link', {
        name: 'Open sharing and public access settings',
      }),
    ).toHaveAttribute('href', '/settings/general');

    await dialog
      .getByRole('button', { name: 'Create share link' })
      .click();
    await expect(dialog.getByText('Share link is ready')).toBeVisible();
  });

  test('share dialog and public view stay inside a narrow viewport', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 768 });
    await page.goto('/dashboards/d1');
    await page.getByRole('button', { name: 'Share', exact: true }).click();
    const dialog = page.getByRole('dialog');
    const dialogBounds = await dialog.evaluate((element) => {
      const rect = element.getBoundingClientRect();
      return {
        left: rect.left,
        right: rect.right,
        width: rect.width,
        viewport: window.innerWidth,
      };
    });
    expect(dialogBounds.left).toBeGreaterThanOrEqual(0);
    expect(dialogBounds.right).toBeLessThanOrEqual(dialogBounds.viewport);
    expect(dialogBounds.width).toBeLessThanOrEqual(dialogBounds.viewport);

    await dialog
      .getByRole('radio', { name: /Anyone with the link/ })
      .click();
    await dialog
      .locator('input[type="password"]')
      .fill('dashboard-secret');
    await dialog
      .getByRole('button', { name: 'Create share link' })
      .click();
    await dialog.getByRole('button', { name: 'Copy link' }).click();
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    await page.setViewportSize({ width: 375, height: 812 });
    await page.goto(copied);
    await expect(page).toHaveURL(/\/shared$/);
    const pageWidth = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      viewport: window.innerWidth,
    }));
    expect(pageWidth.scrollWidth).toBeLessThanOrEqual(pageWidth.viewport);
  });
});
