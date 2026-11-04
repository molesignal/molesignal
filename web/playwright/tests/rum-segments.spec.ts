import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM } from '../fixtures/mockRumData';

const FIREFOX = {
  id: 'firefox',
  name: 'Firefox people',
  conditions: [{ field: 'browser.name', op: 'is', values: ['Firefox'] }],
};

test.describe('RUM segments', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('an application names its segments', async ({ page }) => {
    await page.goto('/rum/applications');
    await page.getByRole('row').filter({ has: page.getByText(MOCK_RUM.applications.web, { exact: true }) }).click();

    const section = page.locator('[data-rum-segments]');

    await section.getByRole('button', { name: 'Add a segment' }).click();
    await section.getByRole('textbox', { name: 'Name of the segment' }).fill('Germans');
    await section.locator('[data-rum-segment-condition]').getByRole('textbox').last().fill('DE');

    const [request] = await Promise.all([
      page.waitForRequest((asked) => asked.method() === 'PATCH' && asked.url().includes('/rum/applications/')),
      page.getByRole('button', { name: 'Save', exact: true }).click(),
    ]);

    const body = request.postDataJSON() as { settings: { segments: Array<{ name: string; conditions: unknown[] }> } };

    expect(body.settings.segments).toEqual([
      expect.objectContaining({
        name: 'Germans',
        conditions: [{ field: 'geo.country_iso_code', op: 'is', values: ['DE'] }],
      }),
    ]);
  });

  test('the analytics pages are read of the segment chosen, and only of it', async ({ page, mockServer }) => {
    await page.route('**/api/v1/rum/applications', async (route) => {
      const response = await route.fetch({ url: `http://127.0.0.1:${mockServer.port}/api/v1/rum/applications` });
      const applications = (await response.json()) as Array<{ id: string; settings: Record<string, unknown> }>;

      await route.fulfill({
        response,
        json: applications.map((application) =>
          application.id === MOCK_RUM.applications.web ? { ...application, settings: { ...application.settings, segments: [FIREFOX] } } : application,
        ),
      });
    });

    await page.goto(`/rum/analytics/paths?app=${MOCK_RUM.applications.web}&time=now-7d..now`);

    const select = page.locator('[data-rum-segment]');

    await expect(select).toHaveAttribute('data-rum-segment', '__everyone__');

    const [narrowed] = await Promise.all([
      page.waitForRequest((asked) => asked.url().includes('/rum/analytics/paths') && decodeURIComponent(asked.url()).includes('browser.name')),
      (async () => {
        await select.click();
        await page.getByRole('option', { name: 'Firefox people' }).click();
      })(),
    ]);

    expect(decodeURIComponent(narrowed.url())).toContain('"op":"in"');
    await expect(page).toHaveURL(/segment=firefox/);

    // The views of the tab keep to it.
    await page.locator('[data-rum-subnavigation="analytics"]').getByRole('link', { name: 'Retention' }).click();
    await expect(page).toHaveURL(/\/rum\/analytics\/retention.*segment=firefox/);
  });

  test('an application without segments has no choice to offer', async ({ page }) => {
    await page.goto(`/rum/analytics/paths?app=${MOCK_RUM.applications.web}&time=now-7d..now`);

    await expect(page.locator('[data-rum-analytics="paths"], [data-rum-analytics]').first()).toBeVisible();
    await expect(page.locator('[data-rum-segment]')).toHaveCount(0);
  });
});
