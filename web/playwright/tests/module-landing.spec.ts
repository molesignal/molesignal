import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const RANGE = { from: NOW - 3_600_000_000, to: NOW };

function red(requests: number, errors: number, p95: number) {
  return {
    request_count: requests,
    error_count: errors,
    error_rate: requests === 0 ? 0 : errors / requests,
    duration_sum_micros: requests * 180_000,
    duration_average_micros: 180_000,
    p50_micros: 120_000,
    p95_micros: p95,
    p99_micros: p95 * 2,
    latency_partial: false,
    exemplars: [],
  };
}

function service(name: string, health: string, requests: number, errors: number, p95: number) {
  const identity = { namespace: 'shop', name, environment: 'prod' };
  return {
    service: identity,
    first_seen_at: RANGE.from,
    last_seen_at: NOW,
    instrumentation: { recent_instance_count: 2 },
    versions: ['1.0.0'],
    red: red(requests, errors, p95),
    health,
    traces: { namespace: 'shop', service: name, environment: 'prod', from: RANGE.from, to: RANGE.to },
  };
}

const APM_META = {
  range: RANGE,
  resolution: 'minute',
  data_quality: { partial: false, gaps: [], overflow_dimensions: [] },
  activation_boundary: false,
};

const APM_OVERVIEW = {
  meta: APM_META,
  red: red(24_000, 480, 420_000),
  trend: Array.from({ length: 12 }, (_, index) => ({
    bucket_at: RANGE.from + index * 60_000_000,
    red: red(1_800 + index * 40, index === 9 ? 180 : 20, 300_000 + index * 10_000),
  })),
  service_health: { healthy: 2, warning: 0, critical: 1, no_traffic: 1 },
  services: [
    service('gateway', 'healthy', 16_000, 40, 90_000),
    service('checkout', 'healthy', 6_000, 60, 420_000),
    service('payments', 'critical', 2_000, 380, 1_200_000),
    service('notifier', 'no_traffic', 0, 0, 0),
  ],
  top_transactions: [],
  top_dependencies: [],
  top_errors: [],
  recent_versions: [],
};

const APM_EMPTY = {
  meta: APM_META,
  red: red(0, 0, 0),
  trend: [],
  service_health: { healthy: 0, warning: 0, critical: 0, no_traffic: 0 },
  services: [],
  top_transactions: [],
  top_dependencies: [],
  top_errors: [],
  recent_versions: [],
};

async function serveApm(page: Page, overview: unknown) {
  await page.route(/\/api\/v1\/apm\/overview(?:\?.*)?$/, (route) => route.fulfill({ json: overview }));
}

const RUM_EMPTY = {
  metrics: { users: 0, sessions: 0, errorFreeRate: 0, lcpP75: 0, inpP75: 0, clsP75: 0 },
  browserDevices: [],
  regions: [],
  facets: { applications: [], environments: [], versions: [], countries: [], devices: [] },
};

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
    profile_type: 'cpu',
    timestamp: NOW - (index + 1) * 60_000_000,
    total_value: 1_000_000_000,
    sample_count: 100,
    duration_nanos: 10_000_000_000,
    unsymbolized: false,
  }));
  await page.route(/\/api\/v1\/profiles(?:\?.*)?$/, (route) => route.fulfill({ json: { profiles } }));
  await page.route(/\/api\/v1\/profiles\/flamegraph(?:\/selection)?(?:\?.*)?$/, (route) =>
    route.fulfill({ json: { flamebearer: FLAMEBEARER, truncated: false, profile_count: count } }),
  );
}

test.describe('Module landing pages', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('APM opens with a verdict on service health and the RED figures with their trends', async ({
    page,
  }) => {
    await serveApm(page, APM_OVERVIEW);
    await page.goto('/apm/overview');

    const pulse = page.locator('[data-apm-pulse]');
    await expect(pulse).toHaveAttribute('data-verdict', 'critical');
    await expect(pulse.getByRole('heading', { level: 2 })).toHaveText('1 service is critical');
    await expect(pulse.getByText('2 of 4 services healthy')).toBeVisible();
    await expect(
      pulse.getByRole('img', { name: 'Service health: Healthy 2, Warning 0, Critical 1, No traffic 1' }),
    ).toBeVisible();

    // Four figures on one surface, each with the shape it had over the window.
    const readouts = pulse.locator('[data-readout]');
    await expect(readouts).toHaveCount(4);
    await expect(pulse.locator('[data-sparkline="bars"]')).toHaveCount(1);
    await expect(pulse.locator('[data-sparkline="line"]')).toHaveCount(3);
    await expect(readouts.nth(1)).toContainText('Error rate');
    await expect(readouts.nth(1)).toContainText('2.00');

    // The services table draws each figure's size beside it.
    const table = page.getByRole('table').first();
    await expect(table.getByText('payments')).toBeVisible();
    await expect(table.locator('[data-meter]')).toHaveCount(12);
  });

  test('APM keeps the time range and the scope in its navigation card', async ({ page }) => {
    await serveApm(page, APM_OVERVIEW);
    await page.goto('/apm/overview');

    const navigation = page.locator('[data-apm-navigation="surface"]');
    const tabs = navigation.getByRole('link', { name: 'Overview' });
    const timeRange = navigation.getByRole('button', { name: /^Time range:/ });
    const namespace = navigation.getByLabel('Namespace');
    await expect(timeRange).toBeVisible();
    await expect(namespace).toBeVisible();

    const [tabBox, timeBox, scopeBox, cardBox] = await Promise.all([
      tabs.boundingBox(),
      timeRange.boundingBox(),
      namespace.boundingBox(),
      navigation.boundingBox(),
    ]);
    // The time range ends the tabs' row; the entities are the row under it.
    expect(Math.abs(timeBox!.y + timeBox!.height / 2 - (tabBox!.y + tabBox!.height / 2))).toBeLessThanOrEqual(2);
    expect(timeBox!.x + timeBox!.width).toBeGreaterThan(cardBox!.x + cardBox!.width - 24);
    expect(scopeBox!.y).toBeGreaterThanOrEqual(tabBox!.y + tabBox!.height);
    // Nothing is squeezed in beside the title any more.
    await expect(page.getByTestId('page-header').getByLabel('Namespace')).toHaveCount(0);
    await expect(page.getByTestId('page-header')).toContainText(
      'Analyze backend performance through services, Transactions, and Traces.',
    );
  });

  test('APM says how to start when no trace has arrived', async ({ page }) => {
    await serveApm(page, APM_EMPTY);
    await page.goto('/apm/overview');

    const firstRun = page.getByTestId('apm-activation');
    await expect(firstRun.getByRole('heading', { level: 2 })).toHaveText('No APM data yet');
    await expect(firstRun.getByRole('link', { name: 'Connect a service' })).toHaveAttribute(
      'href',
      '/datasource/applications/opentelemetry?signal=traces',
    );
    await expect(firstRun.getByRole('listitem')).toHaveCount(3);
    await expect(firstRun.locator('[data-first-run-art] svg')).toBeVisible();
    await expect(page.locator('[data-apm-pulse]')).toHaveCount(0);
  });

  test('RUM opens with the audience and the Core Web Vitals on their scales', async ({ page }) => {
    await page.goto('/rum/overview');

    const pulse = page.locator('[data-rum-pulse]');
    await expect(pulse.locator('[data-readout]')).toHaveCount(3);
    const vitals = pulse.locator('[data-rum-vital]');
    await expect(vitals).toHaveCount(3);
    // The mock reports LCP 2.8 s, INP 180 ms, CLS 0.08.
    await expect(pulse.locator('[data-rum-vital="lcp"]')).toHaveAttribute('data-grade', 'needs_improvement');
    await expect(pulse.locator('[data-rum-vital="inp"]')).toHaveAttribute('data-grade', 'good');
    await expect(pulse.locator('[data-rum-vital="cls"]')).toHaveAttribute('data-grade', 'good');
    await expect(pulse.locator('[data-rum-vital-mark]')).toHaveCount(3);

    // The vitals are stated once: no second panel repeats them.
    await expect(page.getByRole('heading', { name: 'Core Web Vitals' })).toHaveCount(0);

    // What the page is scoped to sits in the navigation card, under the tabs.
    const scope = page.locator('[data-rum-navigation="surface"] [data-rum-scope]');
    await expect(scope.getByRole('combobox')).toHaveCount(5);
    await expect(scope.getByRole('combobox', { name: 'Application' })).toContainText('All applications');
  });

  test('RUM says how to start when no session has arrived', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/overview(?:\?.*)?$/, (route) => route.fulfill({ json: RUM_EMPTY }));
    await page.goto('/rum/overview');

    const firstRun = page.getByTestId('rum-activation');
    await expect(firstRun.getByRole('heading', { level: 2 })).toHaveText(
      'Start collecting real user experience data',
    );
    await expect(firstRun.getByRole('link', { name: 'Web application' })).toHaveAttribute(
      'href',
      '/datasource/recommended/rum',
    );
    await expect(firstRun.getByRole('listitem')).toHaveCount(4);
    // Nothing to scope before the first session.
    await expect(page.locator('[data-rum-scope]')).toHaveCount(0);
    await expect(page.locator('[data-rum-pulse]')).toHaveCount(0);
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
