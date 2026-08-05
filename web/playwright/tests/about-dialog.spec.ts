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
    await mountMockRoutes(page, mockServer.port, {
      token: 'fake-jwt',
    });
    await page.goto('/account/settings/profile');

    await page.getByRole('button', { name: 'Help' }).click();
    await page.getByRole('menuitem', { name: 'About' }).click();

    const dialog = page.getByRole('dialog', { name: 'About MoleSignal' });
    await expect(dialog).toBeVisible();
    await expect(
      dialog.getByRole('heading', { name: 'About MoleSignal' }),
    ).toHaveText('About');
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
    await expect(dialog.getByTestId('about-release-channel')).toHaveText(
      'stable',
    );
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
    expect(diagnostics).toContain('Release channel: stable');
    expect(diagnostics).toContain('Commit Hash: d86fa2d15d68');
    expect(diagnostics).toContain('Build ID: gha-12345-1');
    expect(diagnostics).toContain('Build branch: main');

    await dialog.getByTestId('about-close').click();
    await expect(dialog).toHaveCount(0);
  });
});
