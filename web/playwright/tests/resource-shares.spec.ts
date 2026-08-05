import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

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
        '/api/v1/resource_shares?resource_type=dashboard&resource_id=d1',
      );
      return response.json() as Promise<
        Array<{
          resource_type: string;
          resource_id: string;
          share_mode: string;
        }>
      >;
    });
    expect(rows[0]).toMatchObject({
      resource_type: 'dashboard',
      resource_id: 'd1',
      share_mode: 'authenticated',
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
        '/api/v1/resource_shares?resource_type=dashboard&resource_id=d1',
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
          require_public_report_password: true,
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
            deny_production_public_shares: true,
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
