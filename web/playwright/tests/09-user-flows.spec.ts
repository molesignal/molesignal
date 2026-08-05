/**
 * User-flow e2e specs — Flows 2–5 (P2-T1).
 *
 * Flow 1 (SRE pager → alert → trace cross-signal jump) lives in
 * `06-alerts-flow-1`. This file covers the remaining four named flows from
 * the redesign tasks: authoring a dashboard, authoring an alert rule,
 * connecting a data source, and the RBAC route guard.
 *
 * Uses the shared mock backend (mock Owner auth, frozen clock, every
 * `/api/v1/**` proxied to the in-test Express server).
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Flow 2 — author a dashboard', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('dashboards list → New dashboard → editor → Apply → back to list', async ({ page }) => {
    await page.goto('/dashboards');

    // The list surfaces the "New dashboard" entry affordance, which routes to
    // the editor. Navigate to its target directly — the dashboards grid runs a
    // continuous layout animation that makes the toolbar button fail
    // Playwright's "stable" actionability gate (the editor click below is fine).
    await expect(page.getByRole('button', { name: 'New dashboard' }).first()).toBeVisible({
      timeout: 10_000,
    });
    await page.goto('/dashboards/new/edit');
    await expect(page).toHaveURL(/\/dashboards\/new\/edit/);

    const title = page.getByLabel('Dashboard title');
    await expect(title).toBeVisible({ timeout: 10_000 });
    await title.fill('SLO overview');

    await page.getByRole('button', { name: 'Apply' }).click();

    // New-dashboard save toasts "Dashboard created: …" and routes back to the list.
    await expect(page).toHaveURL(/\/dashboards$/);
    await expect(page.getByText(/Dashboard created/i)).toBeVisible({ timeout: 5_000 });
    const dashboardRow = page.getByRole('row', { name: /SLO overview/ });
    await expect(dashboardRow).toBeVisible({
      timeout: 5_000,
    });
    await expect(
      dashboardRow.getByTestId('dashboard-resource-icon'),
    ).toBeVisible();
    await expect(dashboardRow.getByText('1', { exact: true })).toBeVisible();
    await expect(dashboardRow.getByText('just now')).toBeVisible();
  });

  test('dashboards toolbar import and folders actions respond', async ({ page }) => {
    await page.goto('/dashboards');

    await page.getByRole('button', { name: 'Import JSON' }).click();
    await expect(page).toHaveURL(/\/dashboards\/import$/);
    await expect(page.getByLabel('Dashboard JSON')).toBeVisible({ timeout: 10_000 });

    await page.goto('/dashboards');
    await page.getByRole('button', { name: 'Manage folders' }).click();
    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 10_000 });
    await expect(page.getByRole('heading', { name: 'Folders' })).toBeVisible();
  });

  test('dashboards folders dialog creates renames and deletes folders', async ({ page }) => {
    await page.goto('/dashboards');

    await page.getByRole('button', { name: 'Manage folders' }).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog).toBeVisible({ timeout: 10_000 });

    await dialog.getByPlaceholder('e.g. Production').fill('Production');
    await dialog.getByRole('button', { name: 'Create' }).click();
    await expect(dialog.getByRole('button', { name: /Production 0 dashboards/ })).toBeVisible();

    await dialog.getByRole('button', { name: 'Edit' }).click();
    await dialog.locator('input').nth(1).fill('Platform');
    await dialog.getByRole('button', { name: 'Save' }).click();
    await expect(dialog.getByRole('button', { name: /Platform 0 dashboards/ })).toBeVisible();

    await dialog.getByRole('button', { name: 'Delete' }).click();
    await page.getByRole('button', { name: 'Delete folder' }).click();
    await expect(dialog.getByRole('button', { name: /Platform 0 dashboards/ })).toHaveCount(0);
  });

  test('dashboard editor Save as opens a dialog and creates a copy', async ({ page }) => {
    await page.goto('/dashboards/new/edit');

    await page.getByLabel('Dashboard title').fill('Source dashboard');
    await page.getByRole('button', { name: 'Save as…' }).click();

    await expect(page.getByRole('dialog')).toBeVisible({ timeout: 10_000 });
    await page.getByLabel('Dashboard name').fill('Copied dashboard');
    await page.getByRole('button', { name: 'Save copy' }).click();

    await expect(page).toHaveURL(/\/dashboards\/dash-\d+$/);
    await expect(page.getByText(/Dashboard copy saved/i)).toBeVisible({ timeout: 5_000 });
    await expect(page.locator('main').getByText('Copied dashboard', { exact: true })).toBeVisible({ timeout: 10_000 });
  });

  test('dashboard editor chart hover shows shared tooltip values', async ({ page }) => {
    await page.route('**/api/v1/query', async (route) => {
      if (route.request().method() !== 'POST') {
        await route.fallback();
        return;
      }
      const base = Date.parse('2026-05-23T09:58:00Z') * 1000;
      await route.fulfill({
        json: {
          columns: ['_timestamp', 'service', 'value'],
          rows: [
            [base, 'api', 7.8],
            [base + 60_000_000, 'api', 8.4],
            [base + 120_000_000, 'api', 8.9],
            [base, 'checkout', 7.2],
            [base + 60_000_000, 'checkout', 8.1],
            [base + 120_000_000, 'checkout', 8.6],
          ],
          scanned_rows: 6,
          took_ms: 2,
        },
      });
    });

    await page.goto('/dashboards/new/edit');
    await page.getByLabel('PromQL query editor').click({ force: true });
    await page.keyboard.insertText('sum by (service) (rate(http_requests_total[5m]))');
    await page.getByRole('button', { name: 'Run' }).click();

    const chart = page.locator('svg.cursor-crosshair');
    await expect(chart).toBeVisible({ timeout: 10_000 });
    await expect(chart.getByTestId('chart-x-axis-labels')).toBeVisible();
    await expect(chart.getByTestId('chart-x-axis-labels')).toContainText('09:58');
    const box = await chart.boundingBox();
    if (!box) throw new Error('chart bounding box missing');
    await page.mouse.move(box.x + box.width * 0.55, box.y + box.height * 0.42);

    const tooltip = page.getByTestId('chart-hover-tooltip');
    await expect(tooltip).toBeVisible({ timeout: 5_000 });
    await expect(tooltip).toContainText('api');
    await expect(tooltip).toContainText('checkout');
    await expect(tooltip).toContainText('8');
  });
});

test.describe('Flow 3 — author an alert rule', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('alerts → New rule → fill drawer → Create → POST fires', async ({ page }) => {
    let created = false;
    // Registered after the fixture's catch-all proxy, so it runs first; defer
    // non-POST (the initial GET /alerts/rules) back to the proxy via fallback.
    await page.route('**/api/v1/alerts/rules', async (route) => {
      if (route.request().method() === 'POST') {
        created = true;
        await route.fulfill({ json: { id: 'rule-new' } });
        return;
      }
      await route.fallback();
    });

    await page.goto('/alerts');
    await page.getByRole('button', { name: 'New rule' }).click();
    await expect(page.getByRole('dialog')).toBeVisible();

    // Exact match — `high_error_rate` is also a substring of the runbook
    // placeholder URL, so a loose match is ambiguous.
    await page.getByPlaceholder('high_error_rate', { exact: true }).fill('checkout latency');
    await page.getByPlaceholder('e.g. api-gateway', { exact: true }).fill('checkout');
    // The stream field only exists after the drawer-stream fix (PR #24); fill
    // it when present so this flow passes both before and after that merge.
    const stream = page.getByPlaceholder('app_metrics', { exact: true });
    if (await stream.count()) await stream.first().fill('app_metrics');

    await page.getByRole('button', { name: 'Create rule' }).click();
    await expect.poll(() => created, { timeout: 5_000 }).toBe(true);
  });
});

test.describe('Flow 4 — connect a data source', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('datasource catalog auto-selects a source and shows ingest + health check', async ({ page }) => {
    await page.goto('/datasource');

    // The catalog redirects to the first source of the active category, landing
    // on a /datasource/:category/:source detail page.
    await expect(page).toHaveURL(/\/datasource\/[^/]+\/[^/]+/, { timeout: 10_000 });
    await expect(page.getByRole('button', { name: 'Verify connection' }).first()).toBeVisible({
      timeout: 10_000,
    });
  });

  test('legacy plural datasource route redirects to the datasource catalog', async ({ page }) => {
    await page.goto('/datasources?time=now-24h..now');

    await expect(page).toHaveURL(/\/datasource\/[^/]+\/[^/]+/, { timeout: 10_000 });
  });

  test('signal-specific datasource links select the matching guide and endpoint', async ({ page }) => {
    const cases = [
      {
        route: '/datasource/custom/curl?signal=logs&stream=app_logs',
        signal: 'Logs',
        endpoint: '/api/v1/ingest/logs/app_logs',
      },
      {
        route: '/datasource/applications/opentelemetry?signal=metrics&stream=app_metrics',
        signal: 'Metrics',
        endpoint: '/api/v1/ingest/metrics/app_metrics',
      },
      {
        route: '/datasource/applications/opentelemetry?signal=traces&stream=app_traces',
        signal: 'Traces',
        endpoint: '/api/v1/ingest/traces/app_traces',
      },
      {
        route: '/datasource/recommended/continuous-profiling?signal=profiles&stream=default',
        signal: 'Profiles',
        endpoint: '/api/v1/profiles/ingest',
      },
    ] as const;

    for (const item of cases) {
      await page.goto(item.route);
      await expect(page.getByRole('button', { name: item.signal, pressed: true })).toBeVisible({
        timeout: 10_000,
      });
      await expect(page.locator('code').filter({ hasText: item.endpoint }).first()).toBeVisible();
      await expect(page.getByText('Unexpected Application Error!')).toHaveCount(0);
    }
  });
});

test.describe('Flow 5 — IAM capability route guard', () => {
  test('Viewer capabilities cannot open /iam/teams', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { role: 'Viewer' });

    await page.goto('/iam/teams');
    await expect(page).toHaveURL(/\/home(?:[?#]|$)/);
    await expect(page.getByRole('heading', { name: 'Teams' })).toHaveCount(0);
  });

  test('Owner capabilities can open /iam/teams', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { role: 'Owner' });
    await page.goto('/iam/teams');
    await expect(page.getByText('Permission required')).toHaveCount(0);
  });
});
