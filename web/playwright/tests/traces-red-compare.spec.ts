/**
 * The comparison of the Traces RED view: the failed or the slow requests the
 * query matches against the others, field by field, the fields they share
 * least first; a value narrows the query to it.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { serveExplorerResults } from '../fixtures/mockExplorers';
import { serveTraceRed } from '../fixtures/mockTraceRed';

const LOADED = { timeout: 15_000 };

test('compares the failed requests with the others and narrows the query to a value', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await serveExplorerResults(page);
  const asked = await serveTraceRed(page);
  await page.goto('/traces?tab=red');
  await expect(page.locator('[data-red-group]').first()).toBeVisible(LOADED);

  // Nothing is read until a set is chosen.
  await expect(page.locator('[data-red-compare]')).toBeVisible();
  expect(asked.compare).toHaveLength(0);

  await page.getByRole('combobox', { name: 'Requests to compare with the others' }).click();
  await page.getByRole('option', { name: 'Failed requests' }).click();

  // Failed requests answered 500 or 503, the others 200; the status they are chosen by is not weighed.
  const first = page.locator('[data-red-compare-field]').first();
  await expect(first).toHaveAttribute('data-red-compare-field', 'http.response.status_code', LOADED);
  await expect(first).toContainText('100% apart');
  await expect(page.locator('[data-red-compare-field="status_code"]')).toHaveCount(0);
  await expect(page.locator('[data-red-compare-sample]')).toContainText('chosen');
  expect(asked.compare.at(-1)?.get('errors')).toBe('true');

  await first.locator('[data-red-compare-value="200"] button').click();

  await expect(page.locator('[data-red-step="0"]')).toContainText('http.response.status_code = 200');

  // The slowest twentieth is chosen from the figures of every request.
  await page.getByRole('combobox', { name: 'Requests to compare with the others' }).click();
  await page.getByRole('option', { name: /Slower than p95/ }).click();

  await expect.poll(() => asked.compare.at(-1)?.get('minDuration')).not.toBeNull();
  expect(asked.compare.at(-1)?.get('errors')).toBeNull();
});
