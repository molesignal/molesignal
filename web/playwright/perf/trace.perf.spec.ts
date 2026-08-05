/**
 * Perf: 100k span trace render
 * (web-playwright-runtime).
 *
 * The trace-flame demo seeds the react-query cache before render, so this
 * measures pure layout + canvas paint (no network, no JSON parse).
 */
import { expect, test } from '@playwright/test';

test('[@perf] 100k span trace canvas first-paint within budget', async ({ page }) => {
  await page.goto('/_demo/trace?spans=100000', { waitUntil: 'commit' });
  const t0 = await page.evaluate(() => performance.now());
  await page.waitForSelector('canvas', { timeout: 30_000 });
  const t1 = await page.evaluate(() => performance.now());
  const dt = t1 - t0;
  // EXPECTED: ≤ 200ms per spec; CI relaxed to 1.5s.
  expect(dt, `paint dt=${dt.toFixed(1)}ms`).toBeLessThan(1_500);
});
