import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;

/** An incident as the list answers it; only what Home reads is filled in. */
function incident(id: string, fields: Record<string, unknown>) {
  return {
    id,
    orgId: 'org-1',
    ruleId: 'rule-1',
    status: 'open',
    severity: 'error',
    summary: id,
    assignees: [],
    affectedServices: ['checkout'],
    traceIds: [],
    hostIds: [],
    labels: {},
    createdAt: Date.now() * 1000 - 10 * 60_000_000,
    updatedAt: Date.now() * 1000,
    ...fields,
  };
}

async function mockIncidents(page: Page, incidents: unknown[]) {
  await page.route(/\/api\/v1\/alerts\/incidents(\?.*)?$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: incidents }) : route.fallback(),
  );
}

function red(requestCount: number, errorRate: number, p95Time: number) {
  return {
    requestCount,
    errorCount: Math.round(requestCount * errorRate),
    errorRate,
    durationSum: requestCount * p95Time * 0.4,
    p50Time: p95Time * 0.4,
    p95Time,
    p99Time: p95Time * 1.8,
    latencyPartial: false,
    exemplars: [],
  };
}

function service(name: string, health: string, requestCount: number, errorRate: number, p95Time: number) {
  return {
    service: { namespace: 'shop', name, environment: 'prod' },
    firstSeenAt: NOW - 86_400_000_000,
    lastSeenAt: NOW,
    instrumentation: { recentInstanceCount: 2 },
    versions: ['v1'],
    red: red(requestCount, errorRate, p95Time),
    sparkline: Array.from({ length: 12 }, (_, index) => ({ bucketAt: NOW - (12 - index) * 300_000_000, requestCount: requestCount / 12 + index, errorCount: 0 })),
    health,
    healthReasons: health === 'critical' ? [{ metric: 'error_rate', level: 'critical', value: errorRate, threshold: 0.05 }] : [],
    traces: { namespace: 'shop', service: name, environment: 'prod', from: NOW - 3_600_000_000, to: NOW },
  };
}

/** Every service together over the hour: one failing, one slow, two healthy. */
async function mockApm(page: Page) {
  const services = [
    service('search', 'healthy', 9_000, 0.001, 80_000),
    service('checkout', 'critical', 4_000, 0.12, 2_400_000),
    service('catalog', 'warning', 6_000, 0.01, 900_000),
    service('web', 'healthy', 12_000, 0.002, 120_000),
  ];
  await page.route('**/api/v1/apm/overview**', (route) =>
    route.fulfill({
      json: {
        meta: { range: { from: NOW - 3_600_000_000, to: NOW }, resolution: 'minute', dataQuality: { partial: false, gaps: [], overflowDimensions: [] }, activationBoundary: false },
        red: red(31_000, 0.0213, 640_000),
        trend: Array.from({ length: 12 }, (_, index) => ({ bucketAt: NOW - (12 - index) * 300_000_000, red: red(2_500, 0.01 + index * 0.002, 400_000 + index * 20_000) })),
        serviceHealth: { healthy: 2, warning: 1, critical: 1, no_traffic: 0 },
        services,
        serviceCount: 4,
        topTransactions: [],
        topDependencies: [],
        topErrors: [],
        recentVersions: [],
      },
    }),
  );
}

test.describe('Home', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('opens on the system status: a verdict, the signals and a tile for each thing watched', async ({ page }) => {
    await mockApm(page);
    await page.goto('/home');

    const band = page.getByTestId('home-status-band');
    await expect(band.getByTestId('home-verdict')).toHaveAttribute('data-tone', 'red');
    await expect(band.getByTestId('home-verdict')).toContainText(/\d+ issues/);
    await expect(band.locator('[data-signal="traces"]')).toHaveAttribute('data-state', 'delayed');
    await expect(band.locator('[data-signal="traces"]')).toContainText('Delayed');
    await expect(band.locator('[data-home-tile]')).toHaveCount(6);
    await expect(band.locator('[data-home-tile="services"] [data-home-tile-value]')).toHaveText('2');
    await expect(band.locator('[data-home-tile="services"]')).toContainText('of 4');
    await expect(band.locator('[data-home-tile="error_rate"] [data-home-tile-value]')).toHaveText('2.1%');
    await expect(band.locator('[data-home-tile="p95"] [data-home-tile-value]')).toHaveText('640 ms');
    await expect(band.locator('[data-home-tile="incidents"]')).toHaveAttribute('href', '/alerts/incidents');
  });

  test('lists the services to look at first: the unhealthy ones, then the busiest', async ({ page }) => {
    await mockApm(page);
    await page.goto('/home');

    const rows = page.getByTestId('home-services').locator('[data-home-service]');
    await expect(rows).toHaveCount(4);
    await expect(rows.nth(0)).toHaveAttribute('data-home-service', 'checkout');
    await expect(rows.nth(1)).toHaveAttribute('data-home-service', 'catalog');
    await expect(rows.nth(2)).toHaveAttribute('data-home-service', 'web');
    await expect(rows.nth(0)).toContainText('12.0%');
    await expect(rows.nth(0).getByRole('link', { name: 'checkout' })).toHaveAttribute(
      'href',
      '/apm/services/checkout?namespace=shop&environment=prod',
    );
  });

  test('says there are no traces when APM has none, with the way to connect a service', async ({ page }) => {
    await page.goto('/home');

    await expect(page.getByTestId('home-services-empty')).toContainText('No traces received.');
    await expect(page.getByTestId('home-status-band').locator('[data-home-tile="p95"] [data-home-tile-value]')).toHaveText('—');
  });

  test('lists the incidents going on, the most severe first, and acknowledges from the list', async ({ page }) => {
    await mockIncidents(page, [
      incident('inc-warn', { severity: 'warning', summary: 'Dashboard is logging errors' }),
      incident('inc-critical', { severity: 'critical', status: 'acknowledged', summary: 'Public API answers 502' }),
      incident('inc-error', { severity: 'error', summary: 'Checkout p95 is three times its usual' }),
      incident('inc-done', { severity: 'critical', status: 'resolved', summary: 'Already over' }),
    ]);
    const acknowledged: string[] = [];
    await page.route('**/api/v1/alerts/incidents/*/ack', (route) => {
      acknowledged.push(new URL(route.request().url()).pathname);
      return route.fulfill({ status: 204 });
    });
    await page.goto('/home');

    const rows = page.getByTestId('home-incidents').locator('[data-home-incident]');
    await expect(rows).toHaveCount(3);
    await expect(rows.nth(0)).toContainText('Public API answers 502');
    await expect(rows.nth(0)).toContainText('Acknowledged');
    await expect(rows.nth(1)).toContainText('Checkout p95 is three times its usual');
    await expect(rows.nth(2)).toContainText('Dashboard is logging errors');
    await expect(page.getByTestId('home-status-band').locator('[data-home-tile="incidents"]')).toContainText('2 unacknowledged');

    await rows.nth(1).getByRole('button', { name: 'Acknowledge' }).click();
    await expect.poll(() => acknowledged).toEqual(['/api/v1/alerts/incidents/inc-error/ack']);
  });

  test('says when nothing is going on', async ({ page }) => {
    await mockIncidents(page, []);
    await page.goto('/home');

    await expect(page.getByTestId('home-incidents-empty')).toHaveText('No active incidents');
    await expect(page.getByTestId('home-status-band').locator('[data-home-tile="incidents"] [data-home-tile-value]')).toHaveText('0');
  });

  test('shows each signal arriving, with its trend and when it last arrived', async ({ page }) => {
    await page.goto('/home');

    const signals = page.getByTestId('home-intake').locator('[data-home-signal]');
    await expect(signals).toHaveCount(4);
    await expect(signals.nth(2)).toContainText('Traces');
    await expect(signals.nth(2)).toContainText('Delayed');
    await expect(signals.nth(0).locator('[data-sparkline="bars"]')).toBeVisible();
    await expect(signals.nth(3)).toContainText('No data');
    await expect(page.getByTestId('home-intake')).toContainText('6 of 6 streams receiving');
  });

  test('reads every part again over the window chosen, and keeps it in the address', async ({ page }) => {
    await page.goto('/home');
    await expect(page.getByTestId('home-intake').locator('[data-home-signal]')).toHaveCount(4);

    const intake = page.waitForRequest((request) => request.url().includes('/api/v1/home/overview') && request.url().includes('windowSecs=86400'));
    await page.getByTestId('home-window').click();
    await page.getByRole('option', { name: 'Last 24 hours' }).click();
    await intake;
    await expect(page).toHaveURL(/\/home\?window=24h$/);
  });

  test('degrades instead of crashing when the backend answers with empty bodies', async ({ page }) => {
    const pageErrors: string[] = [];
    page.on('pageerror', (error) => pageErrors.push(error.message));
    for (const path of ['**/api/v1/apm/overview**', '**/api/v1/home/overview**', '**/api/v1/synthetics/availability**']) {
      await page.route(path, (route) => route.fulfill({ json: {} }));
    }
    await page.goto('/home');

    await expect(page.getByTestId('home')).toBeVisible();
    await page.waitForLoadState('networkidle');
    expect(pageErrors).toEqual([]);
  });

  test('gives icon-only chrome controls a 32px target', async ({ page }) => {
    await page.goto('/home');

    for (const testId of ['main-menu-trigger', 'theme-toggle', 'user-menu-trigger', 'home-refresh']) {
      const box = await page.getByTestId(testId).boundingBox();
      expect(box, testId).not.toBeNull();
      expect(box!.width, testId).toBeGreaterThanOrEqual(32);
      expect(box!.height, testId).toBeGreaterThanOrEqual(32);
    }
  });
});

test('sample setup opens the installed dashboard', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route('**/api/v1/streams', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/v1/dashboards', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/v1/onboarding/sample-data', (route) => route.fulfill({
    json: route.request().method() === 'POST'
      ? { loaded: true, totalRows: 196, streams: [], dashboardId: 'sample-dashboard', savedViewIds: ['one', 'two', 'three', 'four'] }
      : { loaded: false },
  }));
  await page.goto('/home');
  await page.locator('[data-home-surface="activation"] button').click();
  await page.getByRole('button', { name: /Load sample data/ }).click();
  await expect(page).toHaveURL(/\/dashboards\/sample-dashboard$/);
});

test.describe('Home for every member', () => {
  test('opens to a member who holds almost nothing, and asks nothing they cannot read', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { capabilityPermissions: ['dashboards.read'] });
    const asked: string[] = [];
    page.on('request', (request) => {
      const { pathname } = new URL(request.url());
      if (/^\/api\/v1\/(alerts\/(incidents|rules)|apm\/overview|synthetics\/(monitors|availability)|home\/overview|schedules)/.test(pathname)) {
        asked.push(pathname);
      }
    });
    await page.goto('/home');

    await expect(page.getByTestId('home-status-band')).toBeVisible();
    await expect(page).toHaveURL(/\/home$/);
    await expect(page.locator('[data-home-tile]')).toHaveCount(0);
    for (const panel of ['home-services', 'home-incidents', 'home-intake', 'home-checks', 'home-oncall']) {
      await expect(page.getByTestId(panel)).toHaveCount(0);
    }
    await page.waitForLoadState('networkidle');
    expect(asked).toEqual([]);
  });

  test('lists incidents to a reader who cannot acknowledge them, without the button', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { capabilityPermissions: ['alerts.incidents.read'] });
    await mockIncidents(page, [incident('inc-error', { summary: 'Checkout is slow' })]);
    await page.goto('/home');

    const row = page.getByTestId('home-incidents').locator('[data-home-incident]');
    await expect(row).toContainText('Checkout is slow');
    await expect(row.getByRole('button', { name: 'Acknowledge' })).toHaveCount(0);
    await expect(row).toContainText('Unacknowledged');
  });
});
