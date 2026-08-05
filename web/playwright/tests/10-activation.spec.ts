/**
 * Activation / Home e2e (P2-T3).
 *
 * The Home page derives an ActivationState from how much the org has set up
 * (streams / dashboards / alerts / pipelines). Previously only vitest unit
 * tests covered this; these specs assert the two ends of the spectrum in a
 * real browser:
 *   - an empty OSS org → activation prompt ("finish the first three steps")
 *   - an active org    → populated streams + "core workflows are active"
 *
 * Data is overridden per-test (rather than via the shared fixture) because the
 * fixture serves the `{ items }` envelope while `streamsApi.list` /
 * `dashboardsApi.list` expect bare arrays — overriding keeps this spec
 * self-contained and deterministic.
 */
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { Page, Route } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const DATA = join(dirname(fileURLToPath(import.meta.url)), '../fixtures/data');
const readItems = (name: string): unknown[] =>
  (JSON.parse(readFileSync(join(DATA, name), 'utf8')) as { items: unknown[] }).items;

const STREAM_ITEMS = readItems('streams.json');
const DASHBOARD_ITEMS = readItems('dashboards.json');

type Json = unknown;

async function overrideHomeData(
  page: Page,
  data: { streams: Json[]; dashboards: Json[]; rules: Json[] },
): Promise<void> {
  const getJson = (body: Json) => async (route: Route) => {
    if (route.request().method() === 'GET') {
      await route.fulfill({ json: body });
      return;
    }
    await route.fallback();
  };
  await page.route('**/api/v1/streams', getJson(data.streams));
  await page.route('**/api/v1/dashboards', getJson(data.dashboards));
  await page.route('**/api/v1/alerts/rules', getJson(data.rules));
  // Home maps the recent-activity feed; the shared catch-all returns `{}` (not
  // an array), which crashes `activity.map`. Serve an empty list explicitly.
  await page.route('**/api/v1/audit**', getJson([]));
}

test.describe('Activation — empty OSS org', () => {
  test('Home prompts activation when nothing is set up', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await overrideHomeData(page, { streams: [], dashboards: [], rules: [] });

    await page.goto('/home');

    // No streams → the streams panel shows its empty state…
    await expect(page.getByText('No streams registered yet.')).toBeVisible({ timeout: 10_000 });
    // …and the activation summary nudges the operator to finish the first steps.
    await expect(page.getByText(/Finish the first three steps/i)).toBeVisible();
  });
});

test.describe('Activation — active org', () => {
  test('Home shows populated streams and a ready activation state', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await overrideHomeData(page, {
      streams: STREAM_ITEMS,
      dashboards: DASHBOARD_ITEMS,
      rules: [{ id: 'rule-1', name: 'p95', enabled: true }],
    });

    await page.goto('/home');

    // Streams render (so the empty state is gone) and ≥3 activation steps are
    // complete (streams + dashboards + alerts) → "core workflows are active".
    await expect(page.getByText('No streams registered yet.')).toHaveCount(0);
    await expect(page.getByText('Core workflows are active')).toBeVisible({ timeout: 10_000 });
  });
});
