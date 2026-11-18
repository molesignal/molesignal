import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('about dialog', () => {
  test('presents product and build information', async ({
    page,
    mockServer,
  }) => {
    await page.addInitScript(() => {
      Object.defineProperty(navigator, 'clipboard', {
        configurable: true,
        value: {
          writeText: async (value: string) => {
            sessionStorage.setItem('copied-about-value', value);
          },
        },
      });
    });
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/account/settings/profile');

    await page.getByRole('button', { name: 'Help' }).click();
    await page.getByRole('menuitem', { name: 'About' }).click();

    const dialog = page.getByRole('dialog', { name: 'About MoleSignal' });
    await expect(dialog).toBeVisible();
    await expect(
      dialog.getByRole('heading', { name: 'About MoleSignal' }),
    ).toHaveClass(/sr-only/);
    const dialogBox = await dialog.boundingBox();
    expect(dialogBox!.width).toBeGreaterThanOrEqual(480);
    expect(dialogBox!.width).toBeLessThanOrEqual(560);

    await expect(dialog.getByText('MoleSignal', { exact: true })).toHaveCount(
      1,
    );
    await expect(
      dialog.getByText(
        'An observability platform for logs, metrics, traces, and performance analysis.',
      ),
    ).toBeVisible();
    await expect(dialog.getByTestId('about-version')).toHaveText(
      'v26.0.0.0',
    );
    await expect(dialog.getByTestId('about-edition')).toHaveText(
      'Enterprise Edition',
    );
    await expect(dialog.getByTestId('about-release-channel')).toHaveCount(0);
    await expect(dialog.getByText('You are up to date')).toBeVisible();
    await expect(dialog.getByText('enterprise', { exact: true })).toHaveCount(
      0,
    );

    await expect(dialog.getByTestId('about-commit')).toHaveText('d86fa2d');
    await expect(dialog.getByTestId('about-build-id')).toHaveText(
      'gha-12345-1',
    );
    await expect(dialog.getByTestId('about-build-time')).toHaveText(
      '2026-07-26 17:36:46 (UTC)',
    );
    await expect(dialog.getByTestId('about-branch')).toHaveText('main');

    await dialog
      .getByRole('button', { name: 'Copy diagnostic info' })
      .click();
    await expect(
      dialog.getByRole('button', { name: 'Diagnostic info copied' }),
    ).toBeVisible();
    const diagnostics = await page.evaluate(() =>
      sessionStorage.getItem('copied-about-value'),
    );
    expect(diagnostics).toContain('Version: v26.0.0.0');
    expect(diagnostics).toContain(
      'Edition type: Enterprise Edition (enterprise)',
    );
    expect(diagnostics).not.toContain('Release channel:');
    expect(diagnostics).toContain('Commit Hash: d86fa2d15d68');
    expect(diagnostics).toContain('Build ID: gha-12345-1');
    expect(diagnostics).toContain('Build branch: main');

    await dialog.getByTestId('about-close').click();
    await expect(dialog).toHaveCount(0);
  });
});

test('release reminder stays red after viewing details and reloading until up to date', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route('**/api/v1/version/updates?*', (route) => route.fulfill({
    json: { currentVersion: '26.0.0.0', latestVersion: '26.2.0.0',
      releaseUrl: 'https://github.com/molesignal/molesignal/releases/tag/v26.2.0.0',
      updateAvailable: true, checkedAt: 1_785_087_406_000_000, status: 'ok' },
  }));
  await page.goto('/account/settings/profile');
  await expect(page.getByTestId('release-update-dot')).toBeVisible();
  await expect(page.getByTestId('release-update-dot')).toHaveClass(/(?:^|\s)bg-red(?:\s|$)/);
  await page.getByRole('button', { name: 'Help', exact: true }).click();
  await page.getByRole('menuitem', { name: /New version available/ }).click();
  const dialog = page.getByRole('dialog', { name: 'About MoleSignal' });
  await expect(dialog.getByText('New version available: v26.2.0.0')).toBeVisible();
  await expect(dialog.getByRole('link', { name: 'Release notes & downloads' }))
    .toHaveAttribute('href', 'https://github.com/molesignal/molesignal/releases/tag/v26.2.0.0');
  await dialog.getByTestId('about-close').click();
  await expect(page.getByTestId('release-update-dot')).toBeVisible();
  await page.reload();
  await page.getByRole('button', { name: 'Help', exact: true }).click();
  await expect(page.getByRole('menuitem', { name: /New version available/ })).toBeVisible();
  await expect(page.getByTestId('release-update-dot')).toBeVisible();
  await page.getByRole('menuitem', { name: /New version available/ }).click();
  await page.route('**/api/v1/version/updates?*', (route) => route.fulfill({
    json: { currentVersion: '26.2.0.0', latestVersion: '26.2.0.0',
      releaseUrl: 'https://github.com/molesignal/molesignal/releases/tag/v26.2.0.0',
      updateAvailable: false, checkedAt: 1_785_087_406_000_000, status: 'ok' },
  }));
  await dialog.getByRole('button', { name: 'Check for updates', exact: true }).click();
  await expect(dialog.getByText('You are up to date')).toBeVisible();
  await dialog.getByTestId('about-close').click();
  await expect(page.getByTestId('release-update-dot')).toHaveCount(0);
  await page.getByRole('button', { name: 'Help', exact: true }).click();
  await expect(page.getByRole('menuitem', { name: /New version available/ })).toHaveCount(0);
});

test('failed checks can be retried without claiming the version is current', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route('**/api/v1/version/updates?*', (route) => route.fulfill({
    json: { currentVersion: '26.0.0.0', latestVersion: null, releaseUrl: null,
      updateAvailable: false, checkedAt: null, status: 'unavailable' },
  }));
  await page.goto('/account/settings/profile');
  await page.getByRole('button', { name: 'Help', exact: true }).click();
  await page.getByRole('menuitem', { name: 'About', exact: true }).click();
  const dialog = page.getByRole('dialog', { name: 'About MoleSignal' });
  await expect(dialog.getByText('Unable to check for updates')).toBeVisible();
  await expect(dialog.getByText('You are up to date')).toHaveCount(0);
  const request = page.waitForRequest((req) => req.url().includes('/version/updates?refresh=true'));
  await dialog.getByRole('button', { name: 'Check for updates', exact: true }).click();
  await request;
  await expect(dialog.getByText('Unable to check for updates')).toBeVisible();
});
