/**
 * 02 — log live tail
 *
 * Verifies the log frame mounts off a stream-result palette pick, the NDJSON
 * stream lands rows into the virtualizer, and the "Live" toggle round-trips
 * without breaking. The dedicated "↓ N new rows" badge requires the second
 * NDJSON batch to land AFTER the user scrolls away — but the Playwright
 * `route.fetch` proxy buffers the response before fulfillment, so the
 * mock's `setTimeout`-gated second batch loses its real-time chunking. The
 * underlying scrollAway / lastSeenCount logic is covered by vitest unit
 * tests on LogStream; the integration here asserts the stream-to-virtualizer
 * path end-to-end and is the appropriate scope for Playwright.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('log live tail', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('rows render from NDJSON stream + scroll-up stays mounted', async ({ page }) => {
    await page.goto('/investigate');

    // Push a log frame via the command palette. Click the stream row
    // directly — cmdk's default selection is unreliable when the typed
    // value fuzzy-matches several static actions.
    await page.keyboard.press('Meta+K');
    await expect(page.getByPlaceholder(/search commands/i)).toBeVisible();
    await page.keyboard.type('logs');
    await page.locator('[cmdk-item][data-value="stream:logs:logs"]').click();

    // Wait for the virtualizer to render the NDJSON rows.
    const firstRow = page.getByRole('row').first();
    await firstRow.waitFor({ state: 'visible', timeout: 10_000 });
    await expect(page.getByText(/^5 rows$/i)).toBeVisible({ timeout: 10_000 });

    // Live toggle round-trip — this exercises the streaming code path
    // (useStreamingLogs unsubscribe + resubscribe) without depending on
    // race-sensitive badge timing.
    await page.getByRole('button', { name: /^Live$/ }).click();
    await expect(page.getByRole('button', { name: /^Live ●$/ })).toBeVisible();

    // Scroll-up inside the container should not unmount the frame.
    const container = page.locator('[aria-label="Log stream"]');
    const box = await container.boundingBox();
    if (box) {
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await page.mouse.wheel(0, -800);
    }
    await expect(firstRow).toBeVisible();
  });
});
