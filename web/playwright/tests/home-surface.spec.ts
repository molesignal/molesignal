import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

/** An incident as the list answers it; only what the workbench reads is filled in. */
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

test.describe('Workbench', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('lays its cards on the canvas without frames: corners and shadows, 16px apart', async ({ page }) => {
    await page.goto('/home');

    await expect(page.getByTestId('workbench')).toBeVisible();
    await expect(page.locator('[data-page-appearance="surface"]')).toBeVisible();
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
    for (const card of ['workbench-recent', 'workbench-resources', 'workbench-todos']) {
      await expect(page.getByTestId(card)).toBeVisible();
    }

    const grid = page.getByTestId('workbench');
    expect(await grid.evaluate((element) => getComputedStyle(element).columnGap)).toBe('16px');

    const sections = page.locator('[data-home-surface="section"]');
    for (const section of await sections.all()) {
      const appearance = await section.evaluate((element) => {
        const styles = getComputedStyle(element);
        return {
          borders: [styles.borderTopWidth, styles.borderRightWidth, styles.borderBottomWidth, styles.borderLeftWidth],
          radius: styles.borderRadius,
          shadow: styles.boxShadow,
        };
      });
      expect(appearance.borders).toEqual(['0px', '0px', '0px', '0px']);
      expect(appearance.radius).not.toBe('0px');
      expect(appearance.shadow).not.toBe('none');
    }
  });

  for (const viewport of [
    { width: 1920, height: 1080 },
    { width: 1440, height: 900 },
    { width: 1280, height: 800 },
  ]) {
    test(`ends both columns on one line and shows whole rows only at ${viewport.width}px`, async ({ page }) => {
      await page.emulateMedia({ reducedMotion: 'reduce' });
      await page.setViewportSize(viewport);
      await page.goto('/home');
      await expect(page.getByTestId('workbench-resources').locator('tbody tr').first()).toBeVisible();

      const bottoms = await page.locator('.wb-grid > .wb-column').evaluateAll((columns) =>
        columns.map((column) => column.lastElementChild!.getBoundingClientRect().bottom),
      );
      expect(bottoms).toHaveLength(2);
      expect(Math.abs(bottoms[0]! - bottoms[1]!)).toBeLessThan(1);

      // The resources list as many rows as fit: none is cut by the edge of its card.
      const cut = await page.getByTestId('workbench-resources').evaluate((card) => {
        const edge = card.getBoundingClientRect().bottom;
        return Array.from(card.querySelectorAll('tbody tr')).filter(
          (row) => row.getBoundingClientRect().bottom > edge + 0.5,
        ).length;
      });
      expect(cut).toBe(0);
    });
  }

  test('lists what waits on the reader, the worst first, and acknowledges from the list', async ({ page }) => {
    await mockIncidents(page, [
      incident('inc-warn', { severity: 'warning', summary: 'Dashboard is logging errors' }),
      incident('inc-error', { severity: 'error', summary: 'Checkout p95 is three times its usual' }),
      incident('inc-theirs', { summary: 'Someone else holds this', assignees: ['someone-else'] }),
    ]);
    const acknowledged: string[] = [];
    await page.route('**/api/v1/alerts/incidents/*/ack', (route) => {
      acknowledged.push(new URL(route.request().url()).pathname);
      return route.fulfill({ status: 204 });
    });
    await page.goto('/home');

    const incidents = page.getByTestId('workbench-todos').locator('[data-todo="incident"]');
    await expect(incidents).toHaveCount(2);
    await expect(incidents.nth(0)).toContainText('Checkout p95 is three times its usual');
    await expect(incidents.nth(1)).toContainText('Dashboard is logging errors');

    await incidents.nth(0).getByRole('button', { name: 'Acknowledge' }).click();
    await expect.poll(() => acknowledged).toEqual(['/api/v1/alerts/incidents/inc-error/ack']);
  });

  test('names a critical incident nobody answered above the workbench, and only that', async ({ page }) => {
    await mockIncidents(page, [
      incident('inc-critical', { severity: 'critical', summary: 'Public API answers 502' }),
      incident('inc-held', { severity: 'critical', status: 'acknowledged', summary: 'Already in hand' }),
      incident('inc-error', { summary: 'Checkout is slow' }),
    ]);
    await page.goto('/home');

    const banner = page.getByText('Critical · 1 firing');
    await expect(banner).toBeVisible();
    await expect(page.getByText('Already in hand')).toHaveCount(0);
  });

  test('says which scope it shows when the one chosen has nothing in it', async ({ page }) => {
    // Dashboards someone else made: nothing in them is the reader's own.
    await page.route('**/api/v1/dashboards', (route) =>
      route.fulfill({
        json: ['Checkout overview', 'Payments SLO'].map((title, index) => ({
          id: `dash-${index}`,
          orgId: 'org-1',
          uid: `dash-${index}`,
          title,
          tags: [],
          model: {},
          version: 1,
          createdAt: 1,
          updatedAt: Date.now() * 1000,
          createdBy: 'someone-else',
        })),
      }),
    );
    await page.goto('/home');

    const resources = page.getByTestId('workbench-resources');
    await expect(resources.locator('tbody tr').first()).toBeVisible();
    // Nothing in the mock is owned by the reader: the list goes on to the next scope that has rows.
    await expect(resources.getByTestId('workbench-resources-fallback')).toContainText('Owned by me');
  });

  test('keeps the places the reader opens, the latest first, for the next visit', async ({ page }) => {
    await page.goto('/alerts/incidents/inc-2292');
    await expect(page.getByText('Checkout p95 at 2.4 s, three times its usual').first()).toBeVisible();
    await page.waitForResponse(
      (response) => response.url().includes('/me/ui-preference/workbench.recent/visits') && response.request().method() === 'PUT',
    );

    await page.goto('/home');
    const recent = page.getByTestId('workbench-recent');
    await expect(recent.getByRole('link').first()).toContainText('Checkout p95 at 2.4 s, three times its usual');
    await expect(recent.getByRole('link').first()).toHaveAttribute('href', '/alerts/incidents/inc-2292');
  });

  test('degrades instead of crashing when the backend answers with empty bodies', async ({ page }) => {
    const pageErrors: string[] = [];
    page.on('pageerror', (error) => pageErrors.push(error.message));
    for (const path of ['**/api/v1/alerts/postmortem-actions/mine', '**/api/v1/streams/usage**', '**/api/v1/streams/budgets']) {
      await page.route(path, (route) => route.fulfill({ json: {} }));
    }
    await page.goto('/home');

    await expect(page.getByTestId('workbench')).toBeVisible();
    await page.waitForLoadState('networkidle');
    expect(pageErrors).toEqual([]);
  });

  test('gives icon-only chrome controls a 32px target', async ({ page }) => {
    await page.goto('/home');

    for (const testId of ['main-menu-trigger', 'theme-toggle', 'user-menu-trigger']) {
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

test.describe('Workbench for every member', () => {
  test('opens to a member who holds almost nothing, and asks nothing they cannot read', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { capabilityPermissions: ['dashboards.read'] });
    const asked: string[] = [];
    page.on('request', (request) => {
      const { pathname } = new URL(request.url());
      if (/^\/api\/v1\/(alerts\/incidents|schedules|notify\/reachability)/.test(pathname)) asked.push(pathname);
    });
    await page.goto('/home');

    const kinds = page.getByTestId('workbench-resources').getByRole('group', { name: 'Kind of resource' });
    await expect(kinds.getByRole('button', { name: /^Dashboards/ })).toBeVisible();
    await expect(page).toHaveURL(/\/home$/);
    await expect(kinds.getByRole('button', { name: /^(Services|Checks)/ })).toHaveCount(0);
    await expect(page.getByTestId('workbench-oncall')).toHaveCount(0);
    await expect(page.getByTestId('workbench-usage')).toHaveCount(0);
    await expect(page.getByTestId('workbench-new')).toHaveCount(0);
    await expect(page.getByTestId('workbench-todos')).not.toContainText('Incidents could not be fetched');
    expect(asked).toEqual([]);
  });

  test('lists incidents to a reader who cannot acknowledge them, without the button', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { capabilityPermissions: ['alerts.incidents.read'] });
    await mockIncidents(page, [incident('inc-error', { summary: 'Checkout is slow' })]);
    await page.goto('/home');

    const row = page.getByTestId('workbench-todos').locator('[data-todo="incident"]');
    await expect(row).toContainText('Checkout is slow');
    await expect(row.getByRole('button', { name: 'Acknowledge' })).toHaveCount(0);
  });
});
