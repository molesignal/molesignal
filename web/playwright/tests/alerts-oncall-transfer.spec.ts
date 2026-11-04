/**
 * Escalation policies, on-call schedules and routing rules as files: each page exports a bundle that
 * names what it refers to, and imports one, tried first and then written.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const SCHEDULES = `format: molesignal.schedules/v1
schedules:
  - name: Nights
    timezone: Asia/Shanghai
    rotations:
      - id: nights
        name: nights
        members: [sre@example.com]
        kind: daily
        startAt: 1790000000000000
`;

test('schedules are imported from a bundle: tried, then written', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const requests: Array<Record<string, unknown>> = [];
  await page.route(/\/api\/v1\/schedules\/import$/, (route) => {
    const body = route.request().postDataJSON() as Record<string, unknown>;
    requests.push(body);
    const applied = body.dryRun === false;
    return route.fulfill({
      json: {
        format: 'molesignal.schedules/v1',
        items: [{ name: 'Nights', action: 'create', ...(applied ? { id: 'sch-nights' } : {}) }],
        applied,
      },
    });
  });

  await page.goto('/alerts/schedules');
  await page.getByTestId('schedule-transfer').click();
  await page.getByTestId('schedule-import').click();

  const drawer = page.getByRole('dialog', { name: 'Import schedules' });
  await drawer.getByRole('textbox', { name: 'Content' }).fill(SCHEDULES);
  await expect(page.getByTestId('schedule-import-apply')).toBeDisabled();
  await page.getByTestId('schedule-import-try').click();

  await expect(drawer.locator('[data-rule-import-report="molesignal.schedules/v1"]')).toContainText('Nights');
  await expect(drawer.locator('[data-import-item="create"]')).toHaveCount(1);
  await page.getByTestId('schedule-import-apply').click();

  await expect.poll(() => requests.length).toBe(2);
  expect(requests[0]).toEqual({ content: SCHEDULES, dryRun: true, createOnly: false });
  expect(requests[1]).toEqual({ content: SCHEDULES, dryRun: false, createOnly: false });
  await expect(page.getByText('Imported 1 schedule', { exact: true })).toBeVisible();
});

test('schedules from another tool: the script that fetches them, and what cannot come over', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const requests: Array<Record<string, unknown>> = [];
  await page.route(/\/api\/v1\/schedules\/import$/, (route) => {
    requests.push(route.request().postDataJSON() as Record<string, unknown>);
    return route.fulfill({
      json: {
        format: 'pagerduty',
        items: [
          {
            name: 'Payments',
            action: 'create',
            findings: [
              { code: 'rotation_ended', params: { rotation: 'Layer 1', at: '2026-10-05 09:30 America/New_York' } },
              {
                code: 'rendered_matched',
                params: { from: '2026-11-01 09:00 America/New_York', to: '2027-05-20 09:00 America/New_York' },
              },
              { code: 'overrides_left_out', params: { count: '2' } },
            ],
          },
          {
            name: 'Platform',
            action: 'error',
            error: 'this schedule cannot be brought over as it is',
            blockers: [{ code: 'participant_team', params: { rotation: 'Day', name: 'ops_team' } }],
            unknownMembers: ['new.one@example.com', 'new.two@example.com'],
          },
          {
            name: 'Shifted',
            action: 'error',
            error: 'this schedule cannot be brought over as it is',
            blockers: [
              {
                code: 'custom_shifts',
                params: {
                  count: '2',
                  at: '2026-12-25 00:00 UTC',
                  until: '2026-12-26 00:00 UTC',
                  user: 'alice@example.com',
                },
              },
            ],
          },
          { name: 'Engineering On-Call', action: 'skip', findings: [{ code: 'shift_based_not_fetched' }] },
        ],
        findings: [{ code: 'schedules_only' }],
        applied: false,
      },
    });
  });

  await page.goto('/alerts/schedules');
  await page.getByTestId('schedule-transfer').click();
  await page.getByTestId('schedule-import').click();

  const drawer = page.getByRole('dialog', { name: 'Import schedules' });

  await drawer.getByText('Moving from Opsgenie or PagerDuty').click();
  await expect(drawer.locator('[data-import-script="opsgenie"]')).toContainText('/v2/schedules/$id/timeline');
  await expect(drawer.getByText('Set OPSGENIE_API_KEY first')).toBeVisible();
  await drawer.getByRole('button', { name: 'PagerDuty' }).click();
  await expect(drawer.locator('[data-import-script="pagerduty"]')).toContainText('shift_based_schedules');
  await expect(drawer.locator('[data-import-script="pagerduty"]')).toContainText(
    '/v3/schedules/$id?since=$since&until=$until&include[]=final_schedule',
  );
  await expect(drawer.getByText('Set PAGERDUTY_API_KEY first')).toBeVisible();

  const content = '{"schedule": {"type": "schedule", "name": "Payments", "schedule_layers": []}}';
  await drawer.getByRole('textbox', { name: 'Content' }).fill(content);
  await page.getByTestId('schedule-import-try').click();

  const report = drawer.locator('[data-rule-import-report="pagerduty"]');
  await expect(report).toContainText('PagerDuty schedules');
  await expect(report.locator('[data-import-finding="rotation_ended"]')).toContainText(
    'Layer 1 ended at 2026-10-05 09:30 America/New_York',
  );
  await expect(report.locator('[data-import-finding="overrides_left_out"]')).toContainText('2 overrides');
  await expect(report.locator('[data-import-finding="participant_team"]')).toContainText('team ops_team takes a turn');
  await expect(report.locator('[data-import-unknown-members]')).toContainText('new.one@example.com');
  await expect(report.locator('[data-import-unknown-members]')).toContainText('new.two@example.com');
  await expect(report.getByText('this schedule cannot be brought over as it is')).toHaveCount(0);
  await expect(report.locator('[data-import-finding="custom_shifts"]')).toContainText(
    '2 custom shifts from now on, the first 2026-12-25 00:00 UTC to 2026-12-26 00:00 UTC, alice@example.com',
  );
  await expect(report.locator('[data-import-item="skip"]')).toContainText('only names this shift-based schedule');
  await expect(report.locator('[data-import-findings]')).toContainText('Escalation policies, routing rules and overrides are left out');
  await expect(page.getByTestId('schedule-import-apply')).toBeDisabled();

  expect(requests).toEqual([{ content, dryRun: true, createOnly: false }]);
});

test('a rotation can end at a set time', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const created: Array<{ rotations: Array<{ startAt: number; endAt?: number | null }> }> = [];
  await page.route(/\/api\/v1\/schedules$/, (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    const body = route.request().postDataJSON() as (typeof created)[number];
    created.push(body);
    return route.fulfill({ json: { ...body, id: 'sch-new', orgId: 'org', createdAt: 0, updatedAt: 0 } });
  });

  await page.goto('/alerts/schedules');
  await page.getByRole('button', { name: 'New schedule' }).first().click();

  const drawer = page.getByRole('dialog', { name: 'New schedule' });
  await drawer.getByRole('textbox', { name: 'Schedule name' }).fill('Holiday cover');
  await drawer.getByRole('checkbox', { name: 'Dana Reyes' }).click();
  await drawer.locator('[data-rotation-end]').getByRole('checkbox', { name: 'Ends at a set time' }).click();
  await expect(drawer.locator('[data-rotation-end]').getByText('End date and time')).toBeVisible();
  await drawer.getByRole('button', { name: 'Create schedule' }).click();

  await expect.poll(() => created.length).toBe(1);
  const rotation = created[0]?.rotations[0];
  expect(rotation?.endAt).toBe((rotation?.startAt ?? 0) + 7 * 24 * 3_600_000_000);
});

test('a routing bundle with an unknown policy is not written', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  await page.route(/\/api\/v1\/alerts\/routing\/rules\/import$/, (route) =>
    route.fulfill({
      json: {
        format: 'molesignal.routing-rules/v1',
        items: [
          { name: 'Payments', action: 'unchanged', id: 'rr-1' },
          { name: 'Search', action: 'error', error: 'no escalation policy is named `Search on-call`' },
        ],
        applied: false,
      },
    }),
  );

  await page.goto('/alerts/routing');
  await page.getByTestId('routing-transfer').click();
  await page.getByTestId('routing-import').click();

  const drawer = page.getByRole('dialog', { name: 'Import routing rules' });
  await drawer
    .getByRole('textbox', { name: 'Content' })
    .fill('{"format":"molesignal.routing-rules/v1","rules":[{"name":"Payments"},{"name":"Search"}]}');
  await page.getByTestId('routing-import-try').click();

  await expect(drawer.locator('[data-import-item="error"]')).toContainText('Search on-call');
  await expect(page.getByTestId('routing-import-apply')).toBeDisabled();
});

test('escalation policies are exported as one bundle', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let asked: string | null = 'not asked';
  await page.route(/\/api\/v1\/alerts\/escalations\/export(\?.*)?$/, (route) => {
    asked = new URL(route.request().url()).searchParams.get('ids');
    return route.fulfill({
      json: { format: 'molesignal.escalation-policies/v1', policies: [{ name: 'Payments' }] },
    });
  });

  await page.goto('/alerts/escalations');
  const download = page.waitForEvent('download');
  await page.getByTestId('policy-transfer').click();
  await page.getByTestId('policy-export').click();

  expect((await download).suggestedFilename()).toMatch(/^molesignal-escalation-policies-\d{4}-\d{2}-\d{2}\.json$/);
  expect(asked).toBeNull();
});
