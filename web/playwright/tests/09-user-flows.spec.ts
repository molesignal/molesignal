/**
 * Product workflow e2e specs covering dashboard and alert authoring, data
 * source setup, and the IAM capability route guard.
 *
 * Uses the shared mock backend (mock Owner auth, frozen clock, every
 * `/api/v1/**` proxied to the in-test Express server).
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Author a dashboard', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('dashboards list → New dashboard → editor → Save → back in the list', async ({ page }) => {
    await page.goto('/dashboards');

    await page.getByRole('button', { name: 'New dashboard' }).click();
    await expect(page).toHaveURL(/\/dashboards\/new\/edit/);

    const title = page.getByLabel('Dashboard title');
    await expect(title).toBeVisible({ timeout: 10_000 });
    await title.fill('SLO overview');

    // A new dashboard starts empty: give it one panel, then leave the panel's
    // editor for the dashboard it sits on.
    await page.getByRole('button', { name: 'Add', exact: true }).click();
    await page.getByRole('menuitem', { name: 'Panel' }).click();
    await expect(page.getByRole('region', { name: 'New panel' })).toBeVisible();
    await page
      .getByRole('button', { name: 'Back to dashboard' })
      .filter({ hasText: 'Back to dashboard' })
      .click();
    await expect(page).toHaveURL(/\/dashboards\/new\/edit$/);

    await page.getByRole('button', { name: 'Save', exact: true }).click();

    // Saving opens the dashboard that was just created.
    await expect(page.getByText('Dashboard saved', { exact: true })).toBeVisible({ timeout: 5_000 });
    await expect(page).toHaveURL(/\/dashboards\/dash-\d+$/);
    await expect(page.getByRole('heading', { level: 1, name: 'SLO overview' })).toBeVisible();

    await page.getByRole('link', { name: 'Back', exact: true }).click();
    await expect(page).toHaveURL(/\/dashboards$/);
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
    const dialog = page.getByRole('dialog', { name: 'Dashboard folders' });
    await expect(dialog).toBeVisible({ timeout: 10_000 });
    const tree = dialog.getByRole('navigation', { name: 'Dashboard folders' });

    await dialog.getByRole('button', { name: 'New folder' }).first().click();
    await expect(dialog.getByRole('heading', { name: 'Create folder' })).toBeVisible();
    await dialog.getByPlaceholder('e.g. Production').fill('Production');
    await dialog.getByRole('button', { name: 'Create', exact: true }).click();
    await expect(page.getByText('Folder created: Production')).toBeVisible();
    // The new folder is in the tree, empty, and open for editing.
    await expect(tree.getByRole('button', { name: 'Production 0' })).toBeVisible();
    await expect(dialog.getByRole('heading', { level: 3, name: 'Production' })).toBeVisible();

    await dialog.getByRole('textbox', { name: 'Name' }).fill('Platform');
    await dialog.getByRole('button', { name: 'Save changes' }).click();
    await expect(page.getByText('Folder updated: Platform')).toBeVisible();
    await expect(tree.getByRole('button', { name: 'Platform 0' })).toBeVisible();
    await expect(tree.getByRole('button', { name: 'Production 0' })).toHaveCount(0);

    await dialog.getByRole('button', { name: 'Delete', exact: true }).click();
    await page.getByRole('button', { name: 'Delete folder' }).click();
    await expect(page.getByText('Folder deleted: Platform')).toBeVisible();
    await expect(tree.getByRole('button', { name: 'Platform 0' })).toHaveCount(0);
  });

  test('a dashboard is copied from its row in the list', async ({ page }) => {
    await page.goto('/dashboards');

    await page.getByRole('button', { name: 'More actions for API latency' }).click();
    await page.getByRole('menuitem', { name: 'Create copy' }).click();

    await expect(
      page.getByText('Dashboard copy created: API latency copy'),
    ).toBeVisible({ timeout: 5_000 });
    // The copy is a dashboard of its own, beside the one it was made from.
    await expect(page.getByRole('row', { name: /^API latency copy/ })).toBeVisible();
    await expect(
      page.getByRole('button', { name: 'More actions for API latency', exact: true }),
    ).toBeVisible();

    await page.getByRole('button', { name: 'More actions for API latency copy' }).click();
    await page.getByRole('menuitem', { name: 'Open' }).click();
    await expect(page).toHaveURL(/\/dashboards\/dash-\d+$/);
    await expect(
      page.getByRole('heading', { level: 1, name: 'API latency copy' }),
    ).toBeVisible({ timeout: 10_000 });
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
    const add = page.getByRole('button', { name: 'Add', exact: true });
    await add.click();
    await page.getByRole('menuitem', { name: 'Panel' }).click();
    // A menu hands the focus back to its button a moment after it closes, and under the fixture's
    // clock that moment can come after the editor is on the page. Text sent before it would be
    // left on the button, so the typing waits for it.
    await expect(add).toBeFocused({ timeout: 10_000 });

    // The panel's live preview runs the query as it is written.
    await page.getByRole('textbox', { name: 'Expression' }).focus();
    await page.keyboard.insertText('sum by (service) (rate(http_requests_total[5m]))');

    const preview = page.getByRole('region', { name: 'New panel' });
    const chart = preview.getByRole('img', { name: 'Dashboard time series' });
    await expect(chart).toBeVisible({ timeout: 10_000 });
    const legend = preview.getByRole('table', { name: 'Series legend' });
    await expect(legend.getByRole('button', { name: '{service="api"}' })).toBeVisible();
    await expect(legend.getByRole('button', { name: '{service="checkout"}' })).toBeVisible();

    const plot = chart.locator('.u-over');
    const box = await plot.boundingBox();
    if (!box) throw new Error('chart bounding box missing');
    // The dashboard shows 09:00–10:00, so the 09:59 samples sit 59/60 of the
    // way across the plot.
    await page.mouse.move(box.x + (box.width * 59) / 60, box.y + box.height * 0.42);

    // One tooltip for the moment under the cursor, with every series' value at it.
    const tooltip = page.getByRole('tooltip');
    await expect(tooltip).toBeVisible({ timeout: 5_000 });
    await expect(tooltip).toContainText('09:59:00');
    await expect(tooltip).toContainText('{service="api"}');
    await expect(tooltip).toContainText('8.4');
    await expect(tooltip).toContainText('{service="checkout"}');
    await expect(tooltip).toContainText('8.1');
  });
});

test.describe('Author an alert rule', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('rules → New rule → fill the workbench → test → Create → POST fires', async ({ page }) => {
    let created: Record<string, unknown> | null = null;
    // Registered after the fixture's catch-all proxy, so it runs first; defer
    // non-POST (the initial GET /alerts/rules) back to the proxy via fallback.
    await page.route('**/api/v1/alerts/rules', async (route) => {
      if (route.request().method() === 'POST') {
        created = route.request().postDataJSON() as Record<string, unknown>;
        await route.fulfill({ json: { id: 'rule-new', ...created } });
        return;
      }
      await route.fallback();
    });

    await page.goto('/alerts/rules');
    await page.getByRole('button', { name: 'New rule' }).click();
    await page.getByRole('menuitem', { name: /Threshold rule/ }).click();
    await expect(page).toHaveURL(/\/alerts\/rules\/new$/);
    await expect(
      page.getByRole('heading', { level: 1, name: 'New alert rule' }),
    ).toBeVisible();

    await page.getByPlaceholder('e.g. api_high_error_rate', { exact: true }).fill('checkout latency');
    await page.getByPlaceholder('e.g. api-gateway', { exact: true }).fill('checkout');
    await page.getByPlaceholder('e.g. app_metrics', { exact: true }).fill('app_metrics');

    // A rule is enabled only once its query has run and returned data.
    const create = page.getByRole('button', { name: 'Create rule', exact: true });
    await expect(create).toBeDisabled();
    await page.getByRole('button', { name: 'Test rule', exact: true }).click();
    await expect(
      page.locator('[data-alert-query-preview]').getByRole('img', { name: 'Time series chart' }),
    ).toBeVisible({ timeout: 10_000 });
    await expect(create).toBeEnabled();

    await create.click();
    await expect.poll(() => created, { timeout: 5_000 }).toMatchObject({
      name: 'checkout latency',
      enabled: true,
      labels: { service: 'checkout' },
      query: {
        language: 'promql',
        stream: { name: 'app_metrics', stream_type: 'metrics' },
      },
    });
    // Saving returns to the rules, and says what was saved.
    await expect(page).toHaveURL(/\/alerts\/rules$/);
    await expect(
      page.getByText('Rule “checkout latency” saved and enabled'),
    ).toBeVisible();
  });
});

test.describe('Connect a data source', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('datasource catalog auto-selects a source and shows intake + health check', async ({ page }) => {
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
        endpoint: '/api/v1/intake/logs/app_logs',
      },
      {
        route: '/datasource/applications/opentelemetry?signal=metrics&stream=app_metrics',
        signal: 'Metrics',
        endpoint: '/api/v1/intake/metrics/app_metrics',
      },
      {
        route: '/datasource/applications/opentelemetry?signal=traces&stream=app_traces',
        signal: 'Traces',
        endpoint: '/api/v1/intake/traces/app_traces',
      },
      {
        route: '/datasource/recommended/continuous-profiling?signal=profiles&stream=default',
        signal: 'Profiles',
        endpoint: '/api/v1/profiles/intake',
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

test.describe('IAM capability route guard', () => {
  test('Viewer capabilities cannot open /iam/teams', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { role: 'Viewer' });

    await page.goto('/iam/teams');
    await expect(page).toHaveURL(/\/home(?:[?#]|$)/);
    await expect(page.getByRole('heading', { name: 'Teams' })).toHaveCount(0);
  });

  test('Owner capabilities can open /iam/teams', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { role: 'Owner' });
    await page.goto('/iam/teams');
    await expect(page).toHaveURL(/\/iam\/teams(?:[?#]|$)/);
    await expect(page.getByRole('heading', { level: 2, name: 'Teams' })).toBeVisible();
    await expect(page.getByText('Permission required')).toHaveCount(0);
  });
});
