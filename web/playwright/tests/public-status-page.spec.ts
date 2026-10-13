import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const WHITE = 'rgb(255, 255, 255)';
const TRANSPARENT = 'rgba(0, 0, 0, 0)';

test.describe('public status page', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { token: 'fake-jwt' });
  });

  test('reads from the verdict down and keeps its three places in the header', async ({ page }) => {
    await page.goto('/status/acme-cloud');

    const verdict = page.getByRole('heading', { name: 'Partial service outage' });
    await expect(verdict).toBeVisible();

    // What is going on stands with the verdict, with the latest word on it and no more.
    const notice = page.locator('[data-status-notice="incident"]');
    await expect(notice.getByRole('link', { name: 'Elevated error rates on the public API' })).toBeVisible();
    await expect(notice.locator('[data-status-latest-update]')).toContainText('rolling it back');
    await expect(notice.getByRole('link', { name: 'Read all 3 updates' })).toBeVisible();
    await expect(page.getByText('We are looking into reports of failed API requests.')).toHaveCount(0);

    // Maintenance that has begun is worded as maintenance; what is only planned comes after.
    const maintenance = page.locator('[data-status-notice="maintenance"]');
    await expect(maintenance).toContainText('Maintenance under way');
    await expect(maintenance).toContainText('In progress');
    await expect(maintenance).not.toContainText('Fix in progress');
    await expect(page.getByRole('region', { name: 'Scheduled maintenance' })).toContainText(
      'Billing export maintenance',
    );

    // The order of the page: verdict, what is planned, every component, what is over.
    const tops = await Promise.all(
      [
        verdict,
        page.getByRole('heading', { name: 'Scheduled maintenance' }),
        page.getByRole('heading', { name: 'System status' }),
        page.getByRole('heading', { name: 'Past incidents' }),
      ].map(async (heading) => (await heading.boundingBox())!.y),
    );
    expect([...tops].sort((left, right) => left - right)).toEqual(tops);

    const places = page.getByRole('navigation', { name: 'Status page navigation' });
    await expect(places.getByRole('link', { name: 'Current status' })).toHaveAttribute('aria-current', 'page');
    // One row of links: nothing in it overflows, so it never grows a scrollbar of its own.
    const overflow = await places.evaluate((nav) => ({
      x: nav.scrollWidth - nav.clientWidth,
      y: nav.scrollHeight - nav.clientHeight,
    }));
    expect(overflow).toEqual({ x: 0, y: 0 });

    await places.getByRole('link', { name: 'Incident history' }).click();
    await expect(page).toHaveURL(/\/status\/acme-cloud\/history\?lang=en-us$/);
    await expect(page.getByRole('heading', { name: 'Incident history' })).toBeVisible();
    await expect(places.getByRole('link', { name: 'Incident history' })).toHaveAttribute('aria-current', 'page');

    const credit = page.getByRole('link', { name: 'Powered by MoleSignal' });
    await expect(credit).toHaveAttribute('href', 'https://molesignal.com');
    await expect(credit).toHaveAttribute('target', '_blank');
  });

  test('draws its controls as controls', async ({ page }) => {
    await page.goto('/status/acme-cloud');

    // The language select says the language and no more: the mark, the name and the arrow stand
    // together, on a fill that tells it from the header it lies on.
    const language = page.getByRole('combobox', { name: 'Status page language' });
    await expect(language).toBeVisible();
    const languageBox = (await language.boundingBox())!;
    expect(languageBox.width).toBeLessThan(140);
    await expect(language).not.toHaveCSS('background-color', WHITE);
    await expect(language).not.toHaveCSS('background-color', TRANSPARENT);

    const subscribe = page.getByRole('button', { name: 'Subscribe' });
    expect((await subscribe.boundingBox())!.height).toBe(languageBox.height);

    await subscribe.click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible();

    // Fields have no border, so the fill is what makes them fields.
    const channel = dialog.getByRole('combobox');
    const address = dialog.getByPlaceholder('you@example.com');
    const feed = dialog.getByRole('link', { name: 'RSS feed' });
    for (const control of [channel, address, feed]) {
      await expect(control).not.toHaveCSS('background-color', WHITE);
      await expect(control).not.toHaveCSS('background-color', TRANSPARENT);
    }
    // The dialog grows into place as it opens, and a box read on screen is scaled with it: the two
    // fields are compared by the height they are laid out at.
    const [channelHeight, addressHeight] = await Promise.all(
      [channel, address].map((control) => control.evaluate((element) => getComputedStyle(element).height)),
    );
    expect(channelHeight).toBe(addressHeight);

    // The dialog is drawn outside the page, and still has the publisher's colour for its button.
    const send = dialog.getByRole('button', { name: 'Send confirmation' });
    await expect(send).not.toHaveCSS('background-color', TRANSPARENT);
    await expect(send).not.toHaveCSS('background-color', WHITE);
  });

  test('says an incident from its latest update back', async ({ page }) => {
    await page.goto('/status/acme-cloud/incidents/inc-api-errors');

    await expect(page.getByRole('heading', { name: 'Elevated error rates on the public API' })).toBeVisible();
    const updates = page.getByRole('region', { name: 'Updates' }).getByRole('listitem');
    await expect(updates).toHaveCount(3);
    await expect(updates.first()).toContainText('Identified');
    await expect(updates.last()).toContainText('We are looking into reports of failed API requests.');

    await page.getByRole('link', { name: 'Back to status page' }).click();
    await expect(page.getByRole('heading', { name: 'Partial service outage' })).toBeVisible();
  });

  test('fits a phone without scrolling sideways', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });

    for (const path of ['/status/acme-cloud', '/status/acme-cloud/history', '/status/acme-cloud/uptime']) {
      await page.goto(path);
      await expect(page.getByRole('navigation', { name: 'Status page navigation' })).toBeVisible();

      const main = page.getByRole('main');
      const sideways = await main.evaluate((element) => element.scrollWidth - element.clientWidth);
      expect(sideways, `${path} scrolls sideways`).toBe(0);
    }
  });
});
