import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults } from '../fixtures/mockExplorers';
import {
  MOCK_INSPECTION,
  MOCK_INSPECTION_RUN,
  setMockInspectionLimits,
  setMockLogInspections,
  updateMockInspectionRun,
} from '../fixtures/mockLogInspections';

const NOW_MICROS = Date.parse(FROZEN_NOW_ISO) * 1000;
const SECOND = 1_000_000;
/** The page reads itself again every few seconds while a run is under way: a change shows by then. */
const FOLLOWED = { timeout: 12_000 };

const front = (page: Page) => page.locator('[data-query-tab-panel][data-active="true"]');
const view = (page: Page, name: string) =>
  front(page).locator('[data-log-result-views]').getByRole('button', { name, exact: true });
const row = (page: Page, id: string) =>
  page.locator('tr', { has: page.locator(`[data-log-inspection="${id}"]`) });
const listedNames = (page: Page) =>
  page.locator('[data-log-inspection] a').evaluateAll((links) => links.map((link) => link.textContent));
const editor = (page: Page) => page.getByRole('dialog');
/** Chooses one of the choices of a field of the form. */
async function choose(page: Page, field: string, choice: string) {
  await editor(page).getByRole('combobox', { name: field, exact: true }).click();
  await page.getByRole('option', { name: choice, exact: true }).click();
}

/** Every save the page sends, as it is sent. */
function watchSaves(page: Page): Array<{ method: string; path: string; body: Record<string, unknown> }> {
  const saves: Array<{ method: string; path: string; body: Record<string, unknown> }> = [];

  page.on('request', (request) => {
    const { pathname } = new URL(request.url());
    const method = request.method();

    if (pathname.startsWith('/api/v1/logs/inspections') && (method === 'POST' || method === 'PUT')) {
      saves.push({
        method,
        path: pathname,
        body: (request.postDataJSON() ?? {}) as Record<string, unknown>,
      });
    }
  });

  return saves;
}

test.describe('scheduled log inspections: from the Logs page', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mountMockRoutes(page, mockServer.port);
    await serveExplorerResults(page);
  });

  test('the inspection tab leads to the inspections that run on a schedule', async ({ page }) => {
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');
    await view(page, 'AI inspection').click();
    await front(page).locator('[data-log-inspections-entry]').click();

    await expect(page).toHaveURL(/\/logs\/inspections$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Scheduled inspections' })).toBeVisible();
    // The page belongs to Logs: its entry stays lit.
    await expect(
      page.getByRole('navigation').getByRole('link', { name: 'Logs', exact: true }).first(),
    ).toHaveAttribute('aria-current', 'page');

    await expect(row(page, MOCK_INSPECTION.payments)).toBeVisible();
    expect(await listedNames(page)).toEqual([
      'Daily audit of application logs',
      'Debug noise (paused)',
      'Payment errors',
      'Weekly gateway review',
    ]);
  });

  test('the query on the page is saved to be inspected on a schedule', async ({ page }) => {
    const saves = watchSaves(page);

    await page.goto(`/logs?q=${encodeURIComponent("level = 'ERROR'")}`);
    await expectExplorerResults(page, 'logs');
    await view(page, 'AI inspection').click();

    const form = front(page).locator('[data-log-inspection="form"]');
    await form.getByLabel('What to attend to').fill('only payment-gateway');
    await form.locator('[data-log-inspection-schedule]').click();

    // The form opens with what is on the page: the stream, the query, the requirements, the model.
    await expect(editor(page)).toContainText('New scheduled inspection');
    await expect(editor(page).getByRole('combobox', { name: 'Stream' })).toContainText('application_logs');
    await expect(editor(page).getByLabel('Query')).toHaveValue("level = 'ERROR'");
    await expect(editor(page).getByLabel('What to attend to')).toHaveValue('only payment-gateway');
    await expect(editor(page).getByRole('combobox', { name: 'AI model' })).toContainText(
      'OpenAI Production · gpt-4o',
    );
    // Whose access it will read the logs with is said beside the button that saves it.
    await expect(editor(page).locator('[data-inspection-run-as]')).toContainText(
      'every run reads the logs with the access you have at that time',
    );

    // Without a name there is nothing to save.
    const save = editor(page).getByRole('button', { name: 'Save inspection' });
    await expect(save).toBeDisabled();
    await editor(page).getByLabel('Name').fill('Gateway errors');
    await save.click();

    await expect(editor(page)).toBeHidden();
    expect(saves).toHaveLength(1);
    expect(saves[0]).toMatchObject({
      method: 'POST',
      path: '/api/v1/logs/inspections',
      body: {
        name: 'Gateway errors',
        stream: 'application_logs',
        statement: "level = 'ERROR'",
        requirements: 'only payment-gateway',
        window_secs: 3_600,
        schedule: { type: 'interval', every_minutes: 60 },
        timezone: 'UTC',
        provider_id: 'openai-gpt-4o',
        model: null,
        notify_when: 'abnormal',
        notify_routes: [],
        language: 'en-us',
        enabled: true,
      },
    });

    // It is among the saved inspections, waiting for its first run.
    await front(page).locator('[data-log-inspections-entry]').click();
    const saved = page.locator('tr', { hasText: 'Gateway errors' });
    await expect(saved).toContainText('Every 1 hour');
    await expect(saved).toContainText('Not run yet');
    await expect(saved).toContainText('No notifications');
  });

  test('a query that matches nothing can still be saved, and the saved ones still reached', async ({ page }) => {
    await page.route('**/api/v1/web/logs', (route) =>
      route.fulfill({ json: { items: [], next_cursor: null, previous_cursor: null, has_more: false } }),
    );
    const listed = page.waitForResponse('**/api/v1/web/logs');
    await page.goto(`/logs?q=${encodeURIComponent("service = 'no-such-service'")}`);
    await listed;
    await expect(front(page).getByTestId('logs-no-data')).toBeVisible();
    await view(page, 'AI inspection').click();

    const form = front(page).locator('[data-log-inspection="form"]');
    await expect(form).toBeVisible();
    // Nothing to inspect right now, and the page says so instead of sending an empty question.
    await expect(form.locator('[data-log-inspection-start]')).toBeDisabled();
    await expect(form).toContainText('The query matches no logs');
    await expect(form.locator('[data-log-inspections-entry]')).toBeVisible();

    await form.locator('[data-log-inspection-schedule]').click();
    await expect(editor(page).getByLabel('Query')).toHaveValue("service = 'no-such-service'");
  });

  test('without a model the tab says so and still leads to what is saved', async ({ page }) => {
    await page.route('**/api/v1/agent/settings/model-providers', (route) => route.fulfill({ json: [] }));
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');
    await view(page, 'AI inspection').click();

    const missing = front(page).getByTestId('log-inspection-no-model');
    await expect(missing).toContainText('Set up and enable an AI model first');
    await missing.getByRole('link', { name: 'Scheduled inspections' }).click();

    await expect(page).toHaveURL(/\/logs\/inspections$/);
    // The saved inspections are listed; none of them can run, and the page says that once.
    await expect(row(page, MOCK_INSPECTION.payments)).toBeVisible();
    await expect(page.locator('[data-log-inspections-no-model]')).toContainText(
      'No AI model is switched on in this organization',
    );
    await expect(page.locator('[data-log-inspection-new]')).toBeDisabled();
  });
});

test.describe('scheduled log inspections: the list', () => {
  test('says what each inspection does, how it last went and when it is next', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/logs/inspections');

    const payments = row(page, MOCK_INSPECTION.payments);
    await expect(payments).toContainText('application_logs');
    await expect(payments).toContainText("level = 'ERROR' AND service = 'payment-gateway'");
    await expect(payments).toContainText('Every 1 hour');
    await expect(payments).toContainText('Covers the last 1 hour');
    await expect(payments).toContainText('Abnormal');
    await expect(payments).toContainText('in 40 minutes');
    await expect(payments).toContainText('2 channels');

    // A run that failed is said as that, not as a verdict.
    await expect(row(page, MOCK_INSPECTION.gateway)).toContainText('Mondays at 08:30');
    await expect(row(page, MOCK_INSPECTION.gateway)).toContainText('Failed');
    await expect(row(page, MOCK_INSPECTION.paused)).toContainText('Not run yet');
    await expect(row(page, MOCK_INSPECTION.paused)).toContainText('Switched off');
    await expect(page.locator('[data-log-inspection-count]')).toHaveText('4 saved; the limit is 20');

    // The table fits the page: nothing is scrolled sideways to reach the controls.
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
    );
    expect(overflow).toBeLessThanOrEqual(0);
    await expect(payments.getByRole('button', { name: 'Delete Payment errors' })).toBeInViewport({ ratio: 1 });
  });

  test('an inspection is switched off and on from its row', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/logs/inspections');

    const payments = row(page, MOCK_INSPECTION.payments);
    const toggle = payments.getByRole('switch', { name: 'Switch Payment errors on or off' });
    await expect(toggle).toBeChecked();

    await toggle.click();
    await expect(toggle).not.toBeChecked();
    await expect(payments).toContainText('Switched off');

    await toggle.click();
    await expect(toggle).toBeChecked();
    await expect(payments).toContainText('in 60 minutes');
  });

  test('a run asked for now is followed from queued to its report', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/logs/inspections');

    const paused = row(page, MOCK_INSPECTION.paused);
    const runNow = paused.getByRole('button', { name: 'Run Debug noise (paused) now' });

    // An inspection that is switched off can still be tried once.
    const queued = page.waitForResponse(
      (response) => response.url().endsWith('/run') && response.request().method() === 'POST',
    );
    await runNow.click();
    const run = (await (await queued).json()) as { id: string };

    await expect(paused).toContainText('Queued');
    await expect(page.locator('[data-log-inspections-live]')).toBeVisible();
    // One run at a time: the control waits for this one.
    await expect(runNow).toBeDisabled();

    // The backend picks it up and finishes it; the list follows without being reloaded.
    await updateMockInspectionRun(mockServer.port, run.id, {
      status: 'running',
      started_at_micros: NOW_MICROS + SECOND,
    });
    await expect(paused).toContainText('Inspecting', FOLLOWED);

    await updateMockInspectionRun(mockServer.port, run.id, {
      status: 'succeeded',
      level: 'attention',
      summary: 'Debug logging is still on for checkout.',
      report: JSON.stringify({ level: 'attention', summary: 'Debug logging is still on for checkout.' }),
      finished_at_micros: NOW_MICROS + 40 * SECOND,
    });
    await expect(paused).toContainText('Needs attention', FOLLOWED);
    await expect(page.locator('[data-log-inspections-live]')).toHaveCount(0, FOLLOWED);
    await expect(runNow).toBeEnabled();
  });

  test('an inspection is changed in the form it was saved with', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    // Registered after the fixture's own routes, so that this one answers.
    await page.route('**/api/v1/notify/connectors', (route) =>
      route.fulfill({
        json: [
          {
            id: 'lark-ops',
            organization_id: 'acme-prod',
            name: 'Lark · Ops',
            connector_type: 'lark_app',
            config: {},
            capabilities: { direct_user: true, group: true, rich_text: true, interactive: false, acknowledgement: false, attachments: false },
            enabled: true,
            status: 'connected',
            created_at: 1,
            updated_at: 1,
          },
        ],
      }),
    );
    const saves = watchSaves(page);
    await page.goto('/logs/inspections');

    await row(page, MOCK_INSPECTION.audit).getByRole('button', { name: 'Edit Daily audit of application logs' }).click();

    await expect(editor(page)).toContainText('Edit scheduled inspection');
    await expect(editor(page).getByLabel('Name')).toHaveValue('Daily audit of application logs');
    await expect(editor(page).getByRole('combobox', { name: 'Stream' })).toContainText('application_logs');
    // It runs as whoever saved it last, and saving it now changes that.
    await expect(editor(page).locator('[data-inspection-run-as]')).toContainText(
      'It runs with the access of Kenji Watanabe now; once saved it runs with yours.',
    );
    await expect(editor(page).locator('[data-inspection-schedule]')).toHaveAttribute(
      'data-inspection-schedule',
      'daily',
    );
    // The time of day is the product's own hour and minute, on the 24-hour clock.
    const time = editor(page).locator('[data-inspection-time]');
    await expect(time).toHaveAttribute('data-inspection-time', '09:00');
    await expect(editor(page).locator('input[type="time"]')).toHaveCount(0);

    // A second channel on the same connector.
    await editor(page).getByRole('button', { name: 'Add channel' }).click();
    await editor(page).getByLabel('Target of channel 2').fill('oc_security');
    await choose(page, 'Hour', '07');
    await choose(page, 'Minute', '30');
    await expect(time).toHaveAttribute('data-inspection-time', '07:30');
    await editor(page).getByRole('button', { name: 'Save', exact: true }).click();

    await expect(editor(page)).toBeHidden();
    expect(saves).toHaveLength(1);
    expect(saves[0]).toMatchObject({
      method: 'PUT',
      path: `/api/v1/logs/inspections/${MOCK_INSPECTION.audit}`,
      body: {
        name: 'Daily audit of application logs',
        schedule: { type: 'daily', hour: 7, minute: 30 },
        notify_when: 'always',
        notify_routes: [
          { connector_id: 'lark-ops', target_type: 'fixed_group', target: 'oc_audit_7f' },
          { connector_id: 'lark-ops', target_type: 'fixed_group', target: 'oc_security' },
        ],
      },
    });
    await expect(row(page, MOCK_INSPECTION.audit)).toContainText('Daily at 07:30');
    await expect(row(page, MOCK_INSPECTION.audit)).toContainText('2 channels');
  });

  test('an inspection is deleted with its reports, once that is confirmed', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/logs/inspections');

    await row(page, MOCK_INSPECTION.gateway).getByRole('button', { name: 'Delete Weekly gateway review' }).click();

    const confirm = page.getByRole('dialog');
    await expect(confirm).toContainText('Delete “Weekly gateway review”?');
    await expect(confirm).toContainText('all its past reports are deleted');
    await confirm.getByRole('button', { name: 'Delete' }).click();

    await expect(row(page, MOCK_INSPECTION.gateway)).toHaveCount(0);
    await expect(page.locator('[data-log-inspection-count]')).toHaveText('3 saved; the limit is 20');
  });

  test('an organization with none is offered its first', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await setMockLogInspections(mockServer.port, []);
    await page.goto('/logs/inspections');

    await expect(page.getByText('No scheduled inspections yet')).toBeVisible();
    await page.locator('[data-log-inspection-new]').click();
    await expect(editor(page)).toContainText('New scheduled inspection');
  });

  test('an organization at the limit of the instance is offered no more', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await setMockInspectionLimits(mockServer.port, { max_per_org: 4 });
    await page.goto('/logs/inspections');

    await expect(page.locator('[data-log-inspection-count]')).toHaveText('4 saved; the limit is 4');
    await expect(page.locator('[data-log-inspection-new]')).toBeDisabled();

    // Deleting one makes room again.
    await row(page, MOCK_INSPECTION.paused).getByRole('button', { name: 'Delete Debug noise (paused)' }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Delete' }).click();
    await expect(page.locator('[data-log-inspection-new]')).toBeEnabled();
  });

  test('a reader sees the inspections and can change none of them', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { role: 'Viewer' });
    await page.goto('/logs/inspections');

    const payments = row(page, MOCK_INSPECTION.payments);
    await expect(payments).toContainText('Abnormal');
    await expect(payments.getByRole('switch')).toBeDisabled();
    await expect(payments.getByRole('button', { name: 'Run Payment errors now' })).toBeDisabled();
    await expect(payments.getByRole('button', { name: 'Edit Payment errors' })).toBeDisabled();
    await expect(payments.getByRole('button', { name: 'Delete Payment errors' })).toBeDisabled();
    await expect(page.locator('[data-log-inspection-new]')).toBeDisabled();

    // The reports are theirs to read.
    await payments.getByRole('link', { name: 'Payment errors' }).click();
    await expect(page.locator('[data-log-inspection-run-report]')).toContainText('Payment errors tripled after 09:10');
    await expect(page.locator('[data-log-inspection-run-now]')).toBeDisabled();
  });

  test('without Mole Agent there is no way to the inspections, and the page is not served', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, {
      capabilityPermissions: ['streams.read', 'streams.query'],
    });
    await serveExplorerResults(page);
    const asked: string[] = [];
    page.on('request', (request) => {
      const { pathname } = new URL(request.url());
      if (pathname.startsWith('/api/v1/logs/inspections')) asked.push(pathname);
    });

    // The tab says what is missing, and offers no way to a page that would turn the reader away.
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');
    await view(page, 'AI inspection').click();
    await expect(front(page).getByTestId('log-inspection-no-permission')).toBeVisible();
    await expect(front(page).locator('[data-log-inspections-entry]')).toHaveCount(0);

    // The address itself leads away, as every page the reader may not open does.
    await page.goto('/logs/inspections');
    await expect(page).not.toHaveURL(/\/logs\/inspections/);
    expect(asked).toEqual([]);
  });
});

test.describe('scheduled log inspections: the reports', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mountMockRoutes(page, mockServer.port);
  });

  test('an inspection opens on its latest report, with what it read and whom it told', async ({ page }) => {
    await page.goto('/logs/inspections');
    await row(page, MOCK_INSPECTION.payments).getByRole('link', { name: 'Payment errors' }).click();

    await expect(page).toHaveURL(new RegExp(`/logs/inspections/${MOCK_INSPECTION.payments}$`));
    await expect(page.getByRole('heading', { level: 1, name: 'Payment errors' })).toBeVisible();
    // What it does is said once, at the head.
    await expect(page.locator('body')).toContainText('Every 1 hour · application_logs · Covers the last 1 hour');
    await expect(page.locator('body')).toContainText('Runs with the access of Dana Reyes');

    const report = page.locator(`[data-log-inspection-run-report="${MOCK_INSPECTION_RUN.abnormal}"]`);
    await expect(report).toContainText('Abnormal');
    await expect(report).toContainText('Payment errors tripled after 09:10');
    await expect(report).toContainText('58 at the peak');
    await expect(report).toContainText('Check the acquirer status page.');
    await expect(report.locator('[data-log-inspection-run-facts]')).toContainText('Scheduled run');
    await expect(report.locator('[data-log-inspection-run-facts]')).toContainText('Took 1 min 12 s');
    await expect(report.locator('[data-log-inspection-run-facts]')).toContainText('19,680 tokens');

    // Only the names of the tools and whether they worked: no query results are kept.
    await expect(report.locator('[data-log-inspection-run-tools] li')).toHaveText([
      'Called get_stream_schema',
      'Called query_logs',
      'query_logs failed',
      'Called search_field_values',
    ]);
    const told = report.locator('[data-log-inspection-run-notifications] li');
    await expect(told.nth(0)).toContainText('Delivered');
    await expect(told.nth(0)).toContainText('oc***7f');
    await expect(told.nth(1)).toContainText('Not delivered');
    await expect(told.nth(1)).toContainText('webhook returned HTTP 502');

    // The logs of the period the report is about are one step away.
    await report.getByRole('link', { name: 'Open the logs of this period' }).click();
    await expect(page).toHaveURL(/\/logs\?.*stream=application_logs/);
  });

  test('the address of a notification opens the run it is about', async ({ page }) => {
    await page.goto(
      `/logs/inspections/${MOCK_INSPECTION.payments}?run=${MOCK_INSPECTION_RUN.timedOut}`,
    );

    const picked = page.locator(`[data-log-inspection-run="${MOCK_INSPECTION_RUN.timedOut}"]`);
    await expect(picked).toHaveAttribute('aria-current', 'true');

    // A run that left no report says why, and what it had read by then.
    const report = page.locator(`[data-log-inspection-run-report="${MOCK_INSPECTION_RUN.timedOut}"]`);
    await expect(report).toContainText('Timed out');
    await expect(report.getByRole('alert')).toContainText('The inspection exceeded its time limit.');
    await expect(report.getByRole('alert')).toContainText('did not finish within 300 seconds');
    await expect(report.locator('[data-log-inspection-run-tools] li')).toHaveText(['Called query_logs']);

    // Another run is a click away, and the address follows.
    await page.locator(`[data-log-inspection-run="${MOCK_INSPECTION_RUN.manualProse}"]`).click();
    await expect(page).toHaveURL(new RegExp(`run=${MOCK_INSPECTION_RUN.manualProse}$`));
    const prose = page.locator(`[data-log-inspection-run-report="${MOCK_INSPECTION_RUN.manualProse}"]`);
    // The model did not keep to the shape: no verdict, and what it wrote is shown as written.
    await expect(prose).toContainText('No verdict');
    await expect(prose).toContainText('The logs look steady over the hour.');
    await expect(prose.locator('[data-log-inspection-run-facts]')).toContainText('Run by Kenji Watanabe');
  });

  test('a run started from the inspection is shown while it waits', async ({ page, mockServer }) => {
    await page.goto(`/logs/inspections/${MOCK_INSPECTION.payments}`);
    await expect(page.locator('[data-log-inspection-run-report]')).toBeVisible();

    const queued = page.waitForResponse(
      (response) => response.url().endsWith('/run') && response.request().method() === 'POST',
    );
    await page.locator('[data-log-inspection-run-now]').click();
    const run = (await (await queued).json()) as { id: string };

    // The new run is the one shown, and it says it is waiting for the backend.
    const report = page.locator(`[data-log-inspection-run-report="${run.id}"]`);
    await expect(report).toHaveAttribute('data-log-inspection-run-report-status', 'queued');
    await expect(report).toContainText('Queued');
    await expect(page.locator('[data-log-inspection-run-now]')).toBeDisabled();

    await updateMockInspectionRun(mockServer.port, run.id, {
      status: 'succeeded',
      level: 'normal',
      summary: 'Nothing unusual in the last hour.',
      report: JSON.stringify({ level: 'normal', summary: 'Nothing unusual in the last hour.' }),
      started_at_micros: NOW_MICROS + SECOND,
      finished_at_micros: NOW_MICROS + 30 * SECOND,
    });

    await expect(report).toHaveAttribute('data-log-inspection-run-report-status', 'succeeded', FOLLOWED);
    await expect(report).toContainText('Nothing unusual in the last hour.');
    await expect(page.locator('[data-log-inspection-run-now]')).toBeEnabled();
  });

  test('an inspection that has not run says so, and that it can be tried', async ({ page }) => {
    await page.goto(`/logs/inspections/${MOCK_INSPECTION.paused}`);

    await expect(page.locator('body')).toContainText('The inspection is switched off; you can still run it once now.');
    await expect(page.locator('[data-log-inspection-run]')).toHaveCount(0);
    await expect(page.locator('[data-log-inspection-run-now]')).toBeEnabled();
  });
});
