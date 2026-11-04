import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const UPLOAD = '/rum/settings/source-maps/upload';

/** The two selects of the form: the artifact type, then the platform it is for. */
const selects = (page: Page) => page.locator('button[role="combobox"]');

async function chooseKind(page: Page, name: string) {
  await selects(page).nth(0).click();
  await page.getByRole('option', { name, exact: true }).click();
}

test.describe('RUM debug artifact upload', () => {
  test('says that the platforms follow the artifact type, and writes each with its first letter in capitals', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto(UPLOAD);

    await expect(selects(page)).toHaveCount(2);
    await expect(page.getByText('The options change with the artifact type.', { exact: true })).toBeVisible();

    await selects(page).nth(1).click();
    await expect(page.getByRole('option')).toHaveText(['Web', 'Flutter']);
  });

  test('keeps a platform of its own when the artifact type changes, for Android and iOS as for the web', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto(UPLOAD);
    await expect(selects(page)).toHaveCount(2);
    await expect(selects(page).nth(1)).toHaveText('Web');

    // What each type is for: the platforms it offers, and the one it starts with.
    const kinds: Array<[string, string, string[]]> = [
      ['Flutter Symbols', 'Android', ['Android', 'iOS']],
      ['Android mapping.txt', 'Android', ['Android']],
      ['Android Native Symbols', 'Android', ['Android']],
      ['Apple dSYM', 'iOS', ['iOS']],
      ['JavaScript Source Map', 'Web', ['Web', 'Flutter']],
    ];

    for (const [kind, platform, offered] of kinds) {
      await chooseKind(page, kind);

      // The platform of the new type is chosen at once, not left empty by the change of its choices.
      await expect(selects(page).nth(1), kind).toHaveText(platform);

      await selects(page).nth(1).click();
      await expect(page.getByRole('option'), kind).toHaveText(offered);
      await page.keyboard.press('Escape');
    }
  });

  test('does not say the name of the list again under the title: the breadcrumb leads back to it', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto(UPLOAD);
    await expect(selects(page)).toHaveCount(2);

    await expect(page.getByText('Source Maps & Symbols', { exact: true })).toHaveCount(0);
    await expect(page.locator('dl')).toHaveCount(0);
    await expect(page.getByRole('link', { name: 'Source maps' }).first()).toBeVisible();
  });

  test('writes the platforms of the uploaded artifacts as the form does, not in capitals', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);

    const artifact = (id: string, kind: string, platform: string, architecture = '') => ({
      id,
      applicationId: 'checkout',
      service: 'app',
      release: '1.4.0',
      kind,
      platform,
      architecture,
      debugId: '',
      filename: `${id}.bin`,
      sizeBytes: 2048,
      checksumSha256: 'c0ffee',
      uploadedAt: 1_760_000_000_000_000,
    });

    await page.route(/\/api\/v1\/debug-artifacts(\?.*)?$/, (route) =>
      route.request().method() === 'GET'
        ? route.fulfill({
            json: [
              artifact('site', 'javascript_sourcemap', 'web'),
              artifact('flutter-web', 'javascript_sourcemap', 'flutter'),
              artifact('libapp', 'android_native_symbols', 'android', 'arm64'),
              artifact('runner', 'apple_dsym', 'ios'),
            ],
          })
        : route.fallback(),
    );
    await page.goto('/rum/settings/source-maps');
    await expect(page.getByText('runner.bin', { exact: true })).toBeVisible();

    for (const name of ['Web', 'Flutter', 'Android', 'iOS']) {
      const cell = page.getByText(name, { exact: true });

      await expect(cell, name).toHaveCount(1);
      await expect(cell, name).toHaveCSS('text-transform', 'none');
    }

    // The architecture stays under its platform.
    await expect(page.getByText('arm64', { exact: true })).toBeVisible();
  });

  test('says the same in Chinese', async ({ page, mockServer }) => {
    await page.addInitScript(() => {
      localStorage.setItem('molesignal-ui-prefs', JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }));
    });
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await page.goto(UPLOAD);

    await expect(selects(page)).toHaveCount(2);
    await expect(page.getByText('可选项随产物类型变化。', { exact: true })).toBeVisible();
    await expect(selects(page).nth(1)).toHaveText('Web');
  });
});
