import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM } from '../fixtures/mockRumData';

const SYNTHETIC = MOCK_RUM.session.synthetic;

test.describe('the sessions tests drove', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('are left out of the figures, and the page says how many', async ({ page }) => {
    await page.goto(`/rum/overview?app=${MOCK_RUM.applications.web}&time=now-24h..now`);

    const note = page.locator('[data-rum-test-traffic]');

    await expect(note).toHaveAttribute('data-rum-test-traffic', '1');
    await expect(note).toContainText('1 test session left out');
  });

  test('are found by asking for them in the explorer', async ({ page }) => {
    await page.goto(`/rum/explorer?type=sessions&time=now-24h..now`);
    await expect(page.locator(`[data-rum-row="${SYNTHETIC}"]`)).toHaveCount(0);

    await page.goto(`/rum/explorer?type=sessions&time=now-24h..now&q=${encodeURIComponent("session.type = 'synthetics'")}`);
    await expect(page.getByText(SYNTHETIC.slice(0, 8)).first()).toBeVisible();
  });

  test('are found by asking for one by its id', async ({ page }) => {
    await page.goto(`/rum/explorer?type=sessions&time=now-24h..now&q=${encodeURIComponent(`session.id = '${SYNTHETIC}'`)}`);
    await expect(page.getByText(SYNTHETIC.slice(0, 8)).first()).toBeVisible();
  });

  test('say on their page which check drove them', async ({ page }) => {
    await page.goto(`/rum/sessions/view/${SYNTHETIC}`);

    const kind = page.locator('[data-rum-session-type="synthetics"]');

    await expect(kind).toContainText('Synthetic check');
    await expect(kind.getByRole('link')).toHaveAttribute('href', `/synthetics/checks/${MOCK_RUM.check.id}/results`);
  });
});
