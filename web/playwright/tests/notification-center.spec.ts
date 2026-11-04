/** The bell: what is waiting on this person, whatever it comes from. */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('the bell', () => {
  test('rings for incidents nobody has answered, and stops once they are read', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/alerts/rules');

    const bell = page.getByTestId('notification-bell');
    await expect(bell).toHaveAttribute('aria-label', /unread/);
    await bell.click();

    const panel = page.getByTestId('notification-panel');
    await expect(panel.getByText('Checkout p95 at 2.4 s, three times its usual')).toBeVisible();
    await expect(panel.getByText('Dashboard is logging 340 errors a minute')).toBeVisible();
    // Someone already answered this one.
    await expect(panel.getByText('Public API answers 16% of requests with 502')).toHaveCount(0);

    const saved = page.waitForRequest(
      (request) => request.url().includes('/me/ui-preference/notifications.read/read') && request.method() === 'PUT',
    );
    await panel.getByRole('button', { name: 'Mark all read' }).click();
    await expect(panel.locator('[data-unread]')).toHaveCount(0);
    await saved;
    await expect(bell).toHaveAttribute('aria-label', 'Notifications');
  });

  test('tells alerts from approvals', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/alerts/rules');
    await page.getByTestId('notification-bell').click();

    const panel = page.getByTestId('notification-panel');
    await panel.getByRole('button', { name: /Approvals/ }).click();
    await expect(panel.locator('[data-notification="alerts"]')).toHaveCount(0);

    await panel.getByRole('button', { name: /Alerts/ }).click();
    await expect(panel.locator('[data-notification="approvals"]')).toHaveCount(0);
    await expect(panel.locator('[data-notification="alerts"]').first()).toBeVisible();
  });

  test('acknowledges an incident where it stands, and opens it when pressed', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/alerts/rules');
    await page.getByTestId('notification-bell').click();

    const panel = page.getByTestId('notification-panel');
    const acknowledged = page.waitForRequest(
      (request) => /\/alerts\/incidents\/inc-2292\/ack/.test(request.url()) && request.method() === 'POST',
    );
    await panel
      .locator('li', { hasText: 'Checkout p95 at 2.4 s' })
      .getByTestId('notification-ack')
      .click();
    await acknowledged;

    await panel.getByText('Dashboard is logging 340 errors a minute').click();
    await expect(page).toHaveURL(/\/alerts\/incidents\/inc-2293$/);
  });
});
