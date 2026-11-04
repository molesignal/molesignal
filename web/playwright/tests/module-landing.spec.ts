import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { mockRumData } from '../fixtures/mockRumData';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const RANGE = { from: NOW - 3_600_000_000, to: NOW };
const SESSIONS = mockRumData(NOW).sessions;

function red(requests: number, errors: number, p95: number) {
  return {
    requestCount: requests,
    errorCount: errors,
    errorRate: requests === 0 ? 0 : errors / requests,
    durationSum: requests * 180_000,
    durationAverage: 180_000,
    p50Time: 120_000,
    p95Time: p95,
    p99Time: p95 * 2,
    latencyPartial: false,
    exemplars: [],
  };
}

function service(name: string, health: string, requests: number, errors: number, p95: number) {
  const identity = { namespace: 'shop', name, environment: 'prod' };
  return {
    service: identity,
    firstSeenAt: RANGE.from,
    lastSeenAt: NOW,
    instrumentation: { recentInstanceCount: 2 },
    versions: ['1.0.0'],
    red: red(requests, errors, p95),
    health,
    traces: { namespace: 'shop', service: name, environment: 'prod', from: RANGE.from, to: RANGE.to },
  };
}

const APM_META = {
  range: RANGE,
  resolution: 'minute',
  dataQuality: { partial: false, gaps: [], overflowDimensions: [] },
  activationBoundary: false,
};

const APM_OVERVIEW = {
  meta: APM_META,
  red: red(24_000, 480, 420_000),
  trend: Array.from({ length: 12 }, (_, index) => ({
    bucketAt: RANGE.from + index * 60_000_000,
    red: red(1_800 + index * 40, index === 9 ? 180 : 20, 300_000 + index * 10_000),
  })),
  serviceHealth: { healthy: 2, warning: 0, critical: 1, no_traffic: 1 },
  services: [
    service('gateway', 'healthy', 16_000, 40, 90_000),
    service('checkout', 'healthy', 6_000, 60, 420_000),
    service('payments', 'critical', 2_000, 380, 1_200_000),
    service('notifier', 'no_traffic', 0, 0, 0),
  ],
  topTransactions: [],
  topDependencies: [],
  topErrors: [],
  recentVersions: [],
};

function apmPage(items: unknown[]) {
  return { meta: APM_META, items, nextCursor: null, previousCursor: null, hasMore: false, sort: 'request_count' };
}

async function serveApm(page: Page, services: unknown[]) {
  await page.route(/\/api\/v1\/apm\/services(?:\?.*)?$/, (route) => route.fulfill({ json: apmPage(services) }));
  await page.route(/\/api\/v1\/apm\/service-definitions(?:\?.*)?$/, (route) => route.fulfill({ json: [] }));
}

const FLAMEBEARER = {
  names: ['total', 'runtime::poll', 'zstd::Encoder::run'],
  // offset, total, self, name
  levels: [
    [0, 100, 0, 0],
    [0, 100, 20, 1],
    [0, 80, 80, 2],
  ],
  numTicks: 100,
  maxSelf: 80,
  units: 'nanoseconds',
};

async function serveProfiles(page: Page, count: number) {
  const profiles = Array.from({ length: count }, (_, index) => ({
    id: `prof-${index}`,
    service: index % 2 === 0 ? 'ingester' : 'querier',
    profileType: 'cpu',
    timestamp: NOW - (index + 1) * 60_000_000,
    totalValue: 1_000_000_000,
    sampleCount: 100,
    durationNanos: 10_000_000_000,
    unsymbolized: false,
  }));
  await page.route(/\/api\/v1\/profiles(?:\?.*)?$/, (route) => route.fulfill({ json: { profiles } }));
  await page.route(/\/api\/v1\/profiles\/flamegraph(?:\/selection)?(?:\?.*)?$/, (route) =>
    route.fulfill({ json: { flamebearer: FLAMEBEARER, truncated: false, profileCount: count } }),
  );
}

test.describe('Module landing pages', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('APM opens on its overview, and its catalog is said in one line over the list', async ({ page }) => {
    await serveApm(page, APM_OVERVIEW.services);
    await page.goto('/apm');
    await expect(page).toHaveURL(/\/apm\/overview$/);

    await page.goto('/apm/services');
    const summary = page.locator('[data-apm-catalog-summary]');
    await expect(summary).toContainText('4 services');
    await expect(summary).toContainText('Critical 1');
    await expect(summary).toContainText('Healthy 2');
    await expect(page.locator('[data-apm-service-row="payments"]')).toBeVisible();
  });

  test('APM keeps the time range in its navigation card', async ({ page }) => {
    await serveApm(page, APM_OVERVIEW.services);
    await page.goto('/apm/services');

    const navigation = page.locator('[data-apm-navigation="surface"]');
    const tabs = navigation.getByRole('link', { name: 'Services' });
    const timeRange = navigation.getByRole('button', { name: /^Time range:/ });
    await expect(navigation.getByRole('link', { name: 'Service map' })).toBeVisible();
    await expect(navigation.getByRole('link', { name: 'Errors' })).toBeVisible();
    await expect(timeRange).toBeVisible();

    const [tabBox, timeBox, cardBox] = await Promise.all([tabs.boundingBox(), timeRange.boundingBox(), navigation.boundingBox()]);
    // The time range ends the tabs' row.
    expect(Math.abs(timeBox!.y + timeBox!.height / 2 - (tabBox!.y + tabBox!.height / 2))).toBeLessThanOrEqual(2);
    expect(timeBox!.x + timeBox!.width).toBeGreaterThan(cardBox!.x + cardBox!.width - 24);
    await expect(page.getByTestId('page-header')).toContainText('Services, their dependencies and their errors');
  });

  test('APM says how to start when no trace has arrived', async ({ page }) => {
    await serveApm(page, []);
    await page.goto('/apm/services');

    const firstRun = page.getByTestId('apm-activation');
    await expect(firstRun.getByRole('heading', { level: 2 })).toHaveText('No APM data yet');
    await expect(firstRun.getByRole('link', { name: 'Connect a service' })).toHaveAttribute(
      'href',
      '/datasource/applications/opentelemetry?signal=traces',
    );
    await expect(firstRun.getByRole('listitem')).toHaveCount(3);
    await expect(firstRun.locator('[data-first-run-art] svg')).toBeVisible();
    await expect(page.locator('[data-apm-catalog-table]')).toHaveCount(0);
  });

  test('RUM opens on its overview, and its explorer is a tab away: the sessions of the day, newest first', async ({
    page,
  }) => {
    await page.goto('/rum');
    // It opens on the day, as the explorer does, and the address says so once the page has taken it.
    await expect(page).toHaveURL(/\/rum\/overview\?time=now-24h\.\.now$/);

    // The tabs name the module and say which of its pages is open.
    const navigation = page.locator('[data-rum-navigation="surface"]');
    await expect(navigation.getByRole('link', { name: 'Overview' })).toHaveAttribute('aria-current', 'page');
    await expect(navigation.getByRole('link', { name: 'Applications' })).toBeVisible();
    await expect(page.locator('[data-rum-pulse-statement]')).toBeVisible();

    // The range in view goes with the reader to the next tab, and the address may say so or not yet.
    await navigation.getByRole('link', { name: 'Explorer' }).click();
    await expect(page).toHaveURL(/\/rum\/explorer(\?time=now-24h\.\.now)?$/);
    await expect(navigation.getByRole('link', { name: 'Explorer' })).toHaveAttribute('aria-current', 'page');

    // The mock holds ten visits, all within the day the explorer opens on; the latest is the first row.
    const rows = page.locator('[data-rum-row]');
    await expect(rows).toHaveCount(SESSIONS.length);
    await expect(rows.first()).toHaveAttribute('data-rum-id', String(SESSIONS[0]?.['session.id']));
    await expect(page.getByRole('tab', { name: /^Sessions/ })).toContainText(String(SESSIONS.length));

    // The country of every row comes from DB-IP, which its licence asks to be credited for.
    await expect(page.locator('[data-rum-geo-credit] a')).toHaveAttribute('href', 'https://db-ip.com');
  });

  test('RUM says how to start when no application exists, and the next step makes one', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/applications(?:\?.*)?$/, (route) => route.fulfill({ json: [] }));
    await page.goto('/rum/explorer');

    const firstRun = page.getByTestId('rum-activation');
    await expect(firstRun.getByRole('heading', { level: 2 })).toHaveText(
      'Start collecting real user experience data',
    );
    await expect(firstRun.getByRole('listitem')).toHaveCount(4);
    // Nothing to ask before the first application: no table, and no credit for lookups it does not show.
    await expect(page.getByTestId('rum-table')).toHaveCount(0);
    await expect(page.locator('[data-rum-geo-credit]')).toHaveCount(0);

    // The explorer opened on the day it looks back over while the applications were being read. The address
    // says so a moment later, on the path it had then: a click made before that would be written over by it.
    await expect(page).toHaveURL(/\/rum\/explorer\?time=now-24h\.\.now$/);
    await firstRun.getByRole('button', { name: 'Create application' }).click();
    // The form is open on the applications page, and a reload of it would not open it again. The range goes
    // with the reader, and the address may say so or not yet.
    await expect(page).toHaveURL(/\/rum\/applications(\?time=now-24h\.\.now)?$/);
    await expect(page.getByPlaceholder('Checkout web')).toBeVisible();
  });

  test('Profiles leads with the hottest function, which finds itself in the flame graph', async ({
    page,
  }) => {
    await serveProfiles(page, 4);
    await page.goto('/profiles');

    const summary = page.locator('[data-profile-summary]');
    const hot = summary.locator('[data-profile-hottest]');
    await expect(hot).toContainText('Hottest');
    await expect(hot).toContainText('zstd::Encoder::run');
    await expect(hot).toContainText('80.0% self');
    // What was merged to find it is said once, on the same line: no band of number cards.
    await expect(summary.locator('[data-profile-scope]')).toContainText('CPU 100 ns');
    await expect(summary.locator('[data-readout]')).toHaveCount(0);

    // The page's parts sit on the canvas as surfaces of their own, not inside one more card.
    await expect(page.locator('[data-list-surface]')).toHaveCount(0);
    const filters = page.getByRole('region', { name: 'Profile analysis filters' });
    const [filterBox, summaryBox] = await Promise.all([filters.boundingBox(), summary.boundingBox()]);
    expect(filterBox!.height).toBeLessThanOrEqual(52);
    expect(summaryBox!.height).toBeLessThanOrEqual(48);
    expect(summaryBox!.y).toBeGreaterThanOrEqual(filterBox!.y + filterBox!.height);

    await hot.click();
    await expect(
      page.locator('[data-profile-view] button[aria-label="Analyze function zstd::Encoder::run"]'),
    ).toHaveClass(/ring-indigo/);
  });

  test('Profiles says how to start when there is nothing to analyze', async ({ page }) => {
    await serveProfiles(page, 0);
    await page.goto('/profiles');

    const firstRun = page.getByTestId('profiles-activation');
    await expect(firstRun.getByRole('heading', { level: 2 })).toHaveText('No profiles in this window');
    await expect(firstRun.getByRole('link', { name: 'Set up OTLP / Pyroscope' })).toHaveAttribute(
      'href',
      '/datasource/recommended/continuous-profiling',
    );
    await expect(firstRun.locator('[data-first-run-art] svg')).toBeVisible();
    await expect(page.locator('[data-profile-summary]')).toHaveCount(0);
  });
});
