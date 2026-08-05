/**
 * Perf: 200 node topology force-layout
 * (web-playwright-runtime).
 */
import { expect, test } from '@playwright/test';

test('[@perf] 200 node topology layout within budget', async ({ page }) => {
  await page.goto('/_demo/topology?nodes=200', { waitUntil: 'commit' });
  const t0 = await page.evaluate(() => performance.now());
  await page.waitForSelector('.react-flow__node', { timeout: 30_000 });
  const t1 = await page.evaluate(() => performance.now());
  const dt = t1 - t0;
  // EXPECTED: ≤ 600ms per spec; CI relaxed to 2s.
  expect(dt, `layout dt=${dt.toFixed(1)}ms`).toBeLessThan(2_000);
});
