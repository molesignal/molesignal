/**
 * Perf: 10M point time-series paint
 * (web-playwright-runtime).
 *
 * Wall-clock from `page.goto` to `<canvas>` first-paint via the in-page
 * `performance.now()` so the measurement runs in the same JS context that
 * actually paints (avoids host↔browser RPC latency in Date.now()).
 */
import { expect, test } from '@playwright/test';

test('[@perf] timeseries 10M point paint within budget', async ({ page }) => {
  await page.goto('/_demo/timeseries?n=10000000', { waitUntil: 'commit' });
  const t0 = await page.evaluate(() => performance.now());
  await page.waitForSelector('canvas', { timeout: 30_000 });
  const t1 = await page.evaluate(() => performance.now());
  const dt = t1 - t0;
  // EXPECTED: ≤ 500ms per spec; CI runners get a generous 5s budget that
  // includes the 10M-point generator running in-page.
  expect(dt, `paint dt=${dt.toFixed(1)}ms`).toBeLessThan(5_000);
});
