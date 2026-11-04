/** The command palette: what is waiting first, then where one was, then everything by what it is. */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('the command palette', () => {
  test('opens on what is waiting, with no word repeated on every row', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/alerts/rules');
    await page.keyboard.press('ControlOrMeta+k');

    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('Waiting on you')).toBeVisible();
    await expect(dialog.getByText('Checkout p95 at 2.4 s, three times its usual')).toBeVisible();
    await expect(dialog.getByText('action', { exact: true })).toHaveCount(0);
  });

  test('acknowledges from the row the arrows are on', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/alerts/rules');
    await page.keyboard.press('ControlOrMeta+k');
    await expect(page.getByRole('dialog').getByText('Waiting on you')).toBeVisible();
    await page.keyboard.press('Home');

    const acknowledged = page.waitForRequest(
      (request) => /\/alerts\/incidents\/inc-2292\/ack/.test(request.url()) && request.method() === 'POST',
    );
    await page.keyboard.press('Alt+KeyA');
    await acknowledged;
  });

  test('remembers where one went', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/alerts/rules');
    await page.keyboard.press('ControlOrMeta+k');

    const dialog = page.getByRole('dialog');
    await dialog.getByRole('combobox').fill('Dashboards');
    await dialog.getByRole('option', { name: 'Dashboards', exact: false }).first().click();
    await expect(page).toHaveURL(/\/dashboards/);

    await page.keyboard.press('ControlOrMeta+k');
    await expect(page.getByRole('dialog').getByText('Recent')).toBeVisible();
  });
});
