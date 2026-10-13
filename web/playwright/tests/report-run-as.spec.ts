import type { Locator, Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW_MICROS = Date.parse(FROZEN_NOW_ISO) * 1_000;

const TAKEOVER = 'After you save, it runs with your access.';
const TOGGLE_HINT = 'Turning this on or off saves the report, which then runs with your access.';

/** A report the way the backend lists it, last saved without a signed-in user. */
function unownedReport(id: string, name: string, format: string) {
  return {
    id,
    org_id: 'acme-prod',
    name,
    dashboard_id: 'd1',
    saved_view_id: null,
    cron: 'every:7d',
    recipients: [{ kind: 'email', target: 'sre@example.com' }],
    format,
    time_range_json: { preset: 'previous-7-days', timezone: 'UTC', description: '' },
    enabled: true,
    run_as_user_id: null,
    run_as_user: null,
    last_run_at_micros: NOW_MICROS - 86_400_000_000,
    created_at_micros: NOW_MICROS - 604_800_000_000,
    updated_at_micros: NOW_MICROS,
  };
}

interface Save {
  id: string;
  format: string;
  enabled: boolean;
}

/**
 * Lists `reports` instead of the mock's own, and keeps what the page saves to them: the mock
 * backend only knows the reports it lists itself.
 */
async function serveReports(page: Page, reports: Array<{ id: string }>): Promise<Save[]> {
  const saves: Save[] = [];

  await page.route('**/api/v1/scheduled_reports', (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: reports }) : route.fallback(),
  );
  await page.route('**/api/v1/scheduled_reports/*', (route) => {
    if (route.request().method() !== 'PUT') return route.fallback();

    const id = new URL(route.request().url()).pathname.split('/').pop() ?? '';
    const body = route.request().postDataJSON() as { format: string; enabled: boolean };
    const report = reports.find((candidate) => candidate.id === id);
    saves.push({ id, format: body.format, enabled: body.enabled });
    return route.fulfill({ json: Object.assign(report ?? {}, body) });
  });

  return saves;
}

/** Opens a report from its row: `action` is what the row offers this reader. */
async function openReport(
  page: Page,
  reportName: string,
  action: 'Edit' | 'View' = 'Edit',
): Promise<Locator> {
  await page
    .getByRole('row', { name: new RegExp(reportName) })
    .getByRole('button', { name: action, exact: true })
    .click();

  const workbench = page.getByRole('dialog');
  await expect(
    workbench.getByRole('heading', {
      name: action === 'Edit' ? 'Edit report' : 'Report details',
    }),
  ).toBeVisible();
  return workbench;
}

test.describe('whose access a scheduled report runs with', () => {
  test("saving someone else's report hands it to the person who saves it", async ({
    page,
    mockServer,
  }) => {
    // An editor schedules reports but may not list the members of the organization. The mock
    // report was last saved by Dana Reyes; the signed-in mock user is Dev User.
    await mountMockRoutes(page, mockServer.port, { role: 'Editor' });
    const memberLists: string[] = [];
    page.on('request', (request) => {
      if (new URL(request.url()).pathname === '/api/v1/users') memberLists.push(request.url());
    });

    await page.goto('/reports');
    const status = page.getByRole('switch', { name: 'Pause Weekly SLO' });
    await expect(status).toBeVisible({ timeout: 15_000 });

    // Enabling or pausing is a save too, and the switch says so before it is used.
    await status.hover();
    await expect(page.getByRole('tooltip')).toHaveText(TOGGLE_HINT);

    const workbench = await openReport(page, 'Weekly SLO');
    const note = workbench.locator('p', { hasText: 'Runs as' });
    // The report names her itself: the member list is not asked, and would have refused.
    await expect(note).toContainText('Dana Reyes');
    await expect(note).toContainText(TAKEOVER);
    await expect(note.getByText('Dana Reyes')).toHaveAttribute('title', 'sre@example.com');
    expect(memberLists).toEqual([]);

    // The statement stands beside the control that saves: on its row, before it.
    const save = workbench.getByRole('button', { name: 'Save changes' });
    const [noteBox, saveBox] = await Promise.all([note.boundingBox(), save.boundingBox()]);
    if (!noteBox || !saveBox) throw new Error('The note or the save button is not laid out');
    expect(noteBox.x + noteBox.width).toBeLessThanOrEqual(saveBox.x);
    expect(
      Math.abs(noteBox.y + noteBox.height / 2 - (saveBox.y + saveBox.height / 2)),
    ).toBeLessThanOrEqual(2);

    // A format that renders no page reads data as no one: there is nothing to state.
    await workbench.getByRole('button', { name: /Delivery$/ }).click();
    await workbench.getByRole('button', { name: /^CSV/ }).click();
    await expect(note).toHaveCount(0);
    await workbench.getByRole('button', { name: /^PDF/ }).click();
    await expect(note).toContainText(TAKEOVER);

    const saved = page.waitForResponse(
      (response) =>
        response.request().method() === 'PUT' &&
        new URL(response.url()).pathname === '/api/v1/scheduled_reports/r1',
    );
    await save.click();
    expect(await (await saved).json()).toMatchObject({
      id: 'r1',
      run_as_user_id: 'dev',
      run_as_user: { id: 'dev', display_name: 'Dev User' },
    });
    await expect(workbench).toHaveCount(0);

    // The report now runs as the person who saved it, and nothing changes hands any more.
    const reopened = await openReport(page, 'Weekly SLO');
    const ownNote = reopened.locator('p', { hasText: 'Runs as' });
    await expect(ownNote).toContainText('Dev User (you)');
    await expect(ownNote).not.toContainText(TAKEOVER);
    await expect(reopened.getByText('Dana Reyes')).toHaveCount(0);
  });

  test('a reader who cannot schedule reports still sees who a report runs as', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, { role: 'Viewer' });

    await page.goto('/reports');
    await expect(page.getByRole('switch', { name: 'Pause Weekly SLO' })).toBeDisabled({
      timeout: 15_000,
    });

    // The row offers the report to look at, not to edit.
    const row = page.getByRole('row', { name: /Weekly SLO/ });
    await expect(row.getByRole('button', { name: 'Edit', exact: true })).toHaveCount(0);

    const details = await openReport(page, 'Weekly SLO', 'View');
    const note = details.locator('p', { hasText: 'Runs as' });
    await expect(note).toHaveText('Runs as Dana Reyes');

    // Nothing here saves: the fields are off, and the only way on is out.
    await expect(details.getByRole('status')).toHaveText(
      'Changing this report requires the reports.schedule permission.',
    );
    await expect(details.getByRole('textbox').first()).toBeDisabled();
    await expect(details.getByRole('button', { name: 'Save changes' })).toHaveCount(0);
    await expect(details.getByRole('button', { name: 'Delete schedule' })).toHaveCount(0);
    await details.getByText('Close', { exact: true }).click();
    await expect(details).toHaveCount(0);
  });

  test('a report that no signed-in user saved fails only where it needs one', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await serveReports(page, [
      unownedReport('r1', 'Token PDF', 'pdf'),
      unownedReport('r2', 'Token CSV', 'csv'),
    ]);

    await page.goto('/reports');
    await expect(page.getByText('Token PDF', { exact: true })).toBeVisible({ timeout: 15_000 });

    const pdf = await openReport(page, 'Token PDF');
    await expect(pdf.getByRole('status')).toHaveText(
      'This report has no user to run as, so its PDF deliveries fail. Save the report to run it with your access.',
    );
    await expect(pdf.getByText('Runs as')).toHaveCount(0);
    await pdf.getByRole('button', { name: 'Cancel' }).click();
    await expect(pdf).toHaveCount(0);

    // CSV renders no page and needs no user: the same missing user is not a problem there.
    const csv = await openReport(page, 'Token CSV');
    await expect(csv.getByRole('button', { name: 'Save changes' })).toBeVisible();
    await expect(csv.getByRole('status')).toHaveCount(0);
    await expect(csv.getByText('Runs as')).toHaveCount(0);
  });

  test('an SVG report stays an SVG report when it is paused or saved', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    const saves = await serveReports(page, [unownedReport('r3', 'Token SVG', 'svg')]);

    await page.goto('/reports');
    const status = page.getByRole('switch', { name: 'Pause Token SVG' });
    await expect(status).toBeVisible({ timeout: 15_000 });

    await status.click();
    await expect.poll(() => saves).toEqual([{ id: 'r3', format: 'svg', enabled: false }]);

    // The workbench shows the format the report has, and SVG needs no user either.
    const workbench = await openReport(page, 'Token SVG');
    await workbench.getByRole('button', { name: /Delivery$/ }).click();
    await expect(workbench.getByRole('button', { name: /^SVG/ })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    await expect(workbench.getByRole('status')).toHaveCount(0);
    await expect(workbench.getByText('Runs as')).toHaveCount(0);

    await workbench.getByRole('button', { name: 'Save changes' }).click();
    await expect.poll(() => saves).toHaveLength(2);
    expect(saves[1]).toEqual({ id: 'r3', format: 'svg', enabled: false });
  });
});
