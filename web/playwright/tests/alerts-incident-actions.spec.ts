/**
 * What people do to an incident besides acknowledging and resolving it: reassign, escalate now, snooze,
 * change the severity, priority or urgency and merge from the incident's menu; the timeline of what was
 * done, with notes; the same steps on several incidents at once from the list; and how an escalation
 * policy notifies again and sets urgency.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.now() * 1000;
const MINUTE = 60_000_000;

const incident = (id: string, summary: string, fields: Record<string, unknown>) => ({
  id,
  orgId: 'acme-prod',
  ruleId: 'rule-checkout-latency',
  escalationPolicyId: 'esc-payments',
  status: 'open',
  severity: 'error',
  summary,
  fingerprint: `fp-${id}`,
  currentStep: 0,
  currentLoop: 0,
  currentStepStartedAt: NOW - 10 * MINUTE,
  assignees: [],
  labels: {},
  annotations: {},
  traceIds: [],
  hostIds: [],
  affectedServices: [],
  triggeringQuery: null,
  createdAt: NOW - 10 * MINUTE,
  ...fields,
});

const CHECKOUT = incident('inc-checkout', 'Checkout p95 above 800 ms', {});
const DASHBOARD = incident('inc-dash', 'Dashboard is logging 340 errors a minute', {
  severity: 'warning',
  escalationPolicyId: 'esc-platform',
});

test('an incident is reassigned, escalated, snoozed, re-rated and merged from its menu', async ({
  page,
  mockServer,
}) => {
  // Five dialogs in a row, each waiting for its toast to go: about 30 seconds on its own.
  test.slow();
  await mountMockRoutes(page, mockServer.port);

  const sent: Record<string, unknown> = {};
  await page.route(/\/api\/v1\/alerts\/incidents(\?.*)?$/, (route) => route.fulfill({ json: [CHECKOUT, DASHBOARD] }));
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-checkout$/, (route) => route.fulfill({ json: CHECKOUT }));
  await page.route(
    /\/api\/v1\/alerts\/incidents\/inc-checkout\/(reassign|escalate|snooze|severity|merge|notes)$/,
    (route) => {
      const action = new URL(route.request().url()).pathname.split('/').pop()!;
      sent[action] = route.request().postDataJSON();
      return route.fulfill({ json: CHECKOUT });
    },
  );

  await page.goto('/alerts/incidents/inc-checkout');
  const more = page.getByTestId('incident-more-actions');
  await expect(more).toBeEnabled();

  // Each step says it is done in a toast over the menu's button, which stays while the pointer is on
  // it: the pointer moves off and the toast goes before the next step.
  const done = async (text: string) => {
    const toast = page.getByText(text, { exact: true });
    await expect(toast).toBeVisible();
    await page.mouse.move(400, 600);
    await expect(toast).toBeHidden();
  };

  // Given to a person, with a note for whoever takes over.
  await more.click();
  await page.getByRole('menuitem', { name: 'Reassign' }).click();
  const reassign = page.getByRole('dialog', { name: 'Reassign' });
  await reassign.getByRole('checkbox', { name: 'Kenji Watanabe' }).check();
  await reassign.getByRole('textbox', { name: 'Note (optional)' }).fill('The 09:04 payments deploy looks related');
  await reassign.getByTestId('incident-assign-submit').click();
  await expect.poll(() => sent.reassign).toEqual({ kind: 'users', userIds: ['u2'] });
  await expect.poll(() => sent.notes).toEqual({ text: 'The 09:04 payments deploy looks related' });
  await expect(reassign).toBeHidden();
  await done('Reassigned');

  // Escalated now: the next step that applies is picked, the current one is named.
  await more.click();
  await page.getByRole('menuitem', { name: 'Escalate now' }).click();
  const escalate = page.getByRole('dialog', { name: 'Escalate now' });
  await expect(escalate.getByRole('radio', { name: 'Step 1 · current' })).not.toBeChecked();
  await expect(escalate.getByRole('radio', { name: 'Step 2' })).toBeChecked();
  await expect(escalate).toContainText('Dana Reyes');
  await escalate.getByTestId('incident-escalate-submit').click();
  await expect.poll(() => sent.escalate).toEqual({ step: 1 });
  await expect(escalate).toBeHidden();
  await done('Escalated');

  // Snoozed for an hour, straight from the menu.
  await more.click();
  await page.getByRole('menuitem', { name: 'Snooze' }).click();
  await page.getByRole('menuitem', { name: '1 hour', exact: true }).click();
  await expect.poll(() => sent.snooze).toEqual({ durationSecs: 3600 });
  await done('Snoozed');

  // Raised to critical.
  await more.click();
  await page.getByRole('menuitem', { name: 'Severity' }).click();
  await page.getByRole('menuitemradio', { name: 'Critical' }).click();
  await expect.poll(() => sent.severity).toEqual({ severity: 'critical' });
  await done('Severity changed');

  // Another active incident merged into this one.
  await more.click();
  await page.getByRole('menuitem', { name: 'Merge' }).click();
  const merge = page.getByRole('dialog', { name: 'Merge incidents' });
  await merge.getByRole('checkbox', { name: 'Dashboard is logging 340 errors a minute' }).check();
  await merge.getByTestId('incident-merge-submit').click();
  await expect.poll(() => sent.merge).toEqual({ sourceIds: ['inc-dash'] });
  await expect(merge).toBeHidden();
});

test('the timeline says what a pipeline changed or could not do, and that a policy did not send', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route(/\/api\/v1\/alerts\/incidents(\?.*)?$/, (route) => route.fulfill({ json: [CHECKOUT] }));
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-checkout$/, (route) => route.fulfill({ json: CHECKOUT }));
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-checkout\/timeline$/, (route) =>
    route.fulfill({
      json: {
        entries: [
          {
            id: 'tl-p1',
            orgId: CHECKOUT.orgId,
            incidentId: CHECKOUT.id,
            at: CHECKOUT.createdAt,
            kind: 'pipeline_processed',
            actor: null,
            detail: {
              pipelineId: 'p-1',
              pipeline: 'Payments routing',
              changes: [
                { field: 'severity', from: 'error', to: 'critical' },
                { field: 'labels.team', to: 'payments' },
                { field: 'labels.host_name', from: 'web-01' },
              ],
              more: 2,
              error: null,
            },
          },
          {
            id: 'tl-p2',
            orgId: CHECKOUT.orgId,
            incidentId: CHECKOUT.id,
            at: CHECKOUT.createdAt,
            kind: 'pipeline_processed',
            actor: null,
            detail: {
              pipelineId: 'p-2',
              pipeline: 'CMDB',
              changes: [],
              more: 0,
              error: 'step `ask`: no answer within 5 seconds',
            },
          },
        ],
        deliveries: [
          {
            id: 'dl-1',
            eventId: `alert.triggered:${CHECKOUT.id}`,
            userId: null,
            target: null,
            connectorId: null,
            stage: 'user_primary',
            status: 'skipped',
            error: 'dropped by alert pipeline "quiet", step "night"',
            createdAt: CHECKOUT.createdAt + 10_000_000,
            deliveredAt: null,
          },
        ],
        merged: [],
      },
    }),
  );

  await page.goto('/alerts/incidents/inc-checkout');

  const traces = page.locator('[data-pipeline-trace]');

  await expect(traces).toHaveCount(2);
  await expect(traces.nth(0)).toContainText('Went through pipeline Payments routing:');
  await expect(traces.nth(0)).toContainText('severity error → critical');
  await expect(traces.nth(0)).toContainText('labels +team=payments, -host_name=web-01');
  await expect(traces.nth(0)).toContainText('and 2 more changes');
  await expect(traces.nth(1)).toContainText('step `ask`: no answer within 5 seconds');
  await expect(page.locator('[data-incident-timeline]')).toContainText(
    'a policy did not send: dropped by alert pipeline "quiet", step "night"',
  );
});

test('the timeline says what was done to an incident and takes a note', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const until = NOW + 50 * MINUTE;
  const handled = {
    ...CHECKOUT,
    status: 'acknowledged',
    severity: 'critical',
    acknowledgedBy: 'u1',
    acknowledgedAt: CHECKOUT.createdAt + 2 * MINUTE,
    assignees: ['u2'],
    assigneesPinned: true,
    snoozedUntil: until,
    snoozedBy: 'u1',
    severitySetBy: 'u2',
  };
  const entry = (id: string, kind: string, minutes: number, actor: string | null, detail: Record<string, unknown>) => ({
    id,
    orgId: 'acme-prod',
    incidentId: CHECKOUT.id,
    at: CHECKOUT.createdAt + minutes * MINUTE,
    kind,
    actor,
    detail,
  });
  const entries = [
    entry('tl-1', 'acknowledged', 2, 'u1', { via: 'sms' }),
    entry('tl-2', 'reassigned', 3, 'u1', { to: { kind: 'users', userIds: ['u2'] }, from: [] }),
    entry('tl-3', 'severity_changed', 4, 'u2', { from: 'error', to: 'critical' }),
    entry('tl-4', 'snoozed', 5, 'u1', { until, durationSecs: 3600 }),
    entry('tl-5', 'merged_in', 6, 'u1', { incidents: ['inc-dash'] }),
  ];

  let note: unknown = null;
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-checkout$/, (route) => route.fulfill({ json: handled }));
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-checkout\/timeline$/, (route) =>
    route.fulfill({
      json: {
        entries,
        deliveries: [
          {
            id: 'dl-1',
            eventId: `alert.triggered:${CHECKOUT.id}`,
            userId: 'u1',
            target: null,
            connectorId: null,
            stage: 'delivered',
            status: 'sent',
            error: null,
            createdAt: CHECKOUT.createdAt + 10_000_000,
            deliveredAt: CHECKOUT.createdAt + 11_000_000,
          },
        ],
        merged: [
          {
            id: 'inc-dash',
            summary: DASHBOARD.summary,
            status: 'open',
            severity: 'warning',
            createdAt: DASHBOARD.createdAt,
            resolvedAt: null,
          },
        ],
      },
    }),
  );
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-checkout\/notes$/, (route) => {
    note = route.request().postDataJSON();
    entries.push(entry('tl-6', 'note', 7, 'u1', { text: 'Rolled back the edge proxy', via: 'web' }));
    return route.fulfill({ json: entries[entries.length - 1] });
  });

  await page.goto('/alerts/incidents/inc-checkout');

  const timeline = page.locator('[data-incident-timeline]');
  const rows = timeline.locator('[data-timeline-row]');
  await expect(rows).toHaveCount(7);
  await expect(rows.nth(1)).toContainText('Notified Dana Reyes');
  await expect(rows.nth(2)).toContainText('Dana Reyes acknowledged (by SMS reply)');
  await expect(rows.nth(3)).toContainText('Dana Reyes reassigned to Kenji Watanabe');
  await expect(rows.nth(4)).toContainText('Kenji Watanabe changed the severity from Error to Critical');
  await expect(rows.nth(5)).toContainText('Dana Reyes snoozed until');
  await expect(rows.nth(6)).toContainText('Dana Reyes merged 1 incident into this one');
  await expect(page.locator('[data-incident-merged]')).toContainText(DASHBOARD.summary);

  // How the incident is held now: snoozed, and its severity no longer up to the rule.
  const handling = page.locator('[data-incident-handling]');
  await expect(handling).toContainText('Snoozed until');
  await expect(handling).toContainText('Severity set by Kenji Watanabe; the rule no longer changes it');

  // A note goes on the timeline.
  const box = page.getByRole('textbox', { name: 'Note', exact: true });
  await box.fill('Rolled back the edge proxy');
  await page.getByTestId('incident-add-note').click();
  await expect.poll(() => note).toEqual({ text: 'Rolled back the edge proxy' });
  await expect(rows).toHaveCount(8);
  await expect(rows.last()).toContainText('Dana Reyes added a note');
  await expect(rows.last()).toContainText('Rolled back the edge proxy');
  await expect(box).toHaveValue('');
});

test('several incidents are handled at once from the list', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const requests: unknown[] = [];
  await page.route(/\/api\/v1\/alerts\/incidents(\?.*)?$/, (route) => route.fulfill({ json: [CHECKOUT, DASHBOARD] }));
  await page.route(/\/api\/v1\/alerts\/incidents\/bulk$/, (route) => {
    const body = route.request().postDataJSON() as { ids: string[] };
    requests.push(body);
    return route.fulfill({ json: { results: body.ids.map((id) => ({ id })) } });
  });

  await page.goto('/alerts/incidents');
  await expect(page.locator('tbody tr')).toHaveCount(2);
  const bar = page.getByRole('toolbar', { name: 'Bulk actions' });
  await expect(bar).toBeHidden();

  // Both ticked, acknowledged together; the selection clears once it is done.
  await page.getByRole('checkbox', { name: `Select ${CHECKOUT.summary}` }).check();
  await page.getByRole('checkbox', { name: `Select ${DASHBOARD.summary}` }).check();
  await expect(bar).toContainText('2 selected');
  await bar.getByRole('button', { name: 'Acknowledge' }).click();
  await expect.poll(() => requests[0]).toEqual({ ids: ['inc-checkout', 'inc-dash'], action: 'acknowledge' });
  await expect(bar).toBeHidden();

  // All of them given to a team.
  await page.getByRole('checkbox', { name: 'Select all' }).check();
  await bar.getByRole('button', { name: 'Reassign' }).click();
  const dialog = page.getByRole('dialog', { name: 'Reassign' });
  await dialog.getByRole('combobox', { name: 'Reassign to' }).click();
  await page.getByRole('option', { name: 'Team' }).click();
  await dialog.getByRole('combobox', { name: 'Team', exact: true }).click();
  await page.getByRole('option', { name: 'Payments' }).click();
  await dialog.getByTestId('incident-assign-submit').click();
  await expect.poll(() => requests[1]).toEqual({
    ids: ['inc-checkout', 'inc-dash'],
    action: 'reassign',
    to: { kind: 'team', teamId: 'team-payments' },
  });
  await expect(dialog).toBeHidden();

  // Severity, for the ones still ticked.
  await bar.getByRole('button', { name: 'Adjust' }).click();
  await page.getByRole('menuitem', { name: 'Severity' }).click();
  await page.getByRole('menuitem', { name: 'Warning' }).click();
  await expect.poll(() => requests[2]).toEqual({
    ids: ['inc-checkout', 'inc-dash'],
    action: 'set_severity',
    severity: 'warning',
  });

  // Then a priority for both of them, from the same menu: done, the selection cleared, so they are ticked again.
  await expect(bar).toBeHidden();
  await page.getByRole('checkbox', { name: 'Select all' }).check();
  await bar.getByRole('button', { name: 'Adjust' }).click();
  await page.getByRole('menuitem', { name: 'Priority' }).click();
  await page.getByRole('menuitem', { name: 'P2' }).click();
  await expect.poll(() => requests[3]).toEqual({
    ids: ['inc-checkout', 'inc-dash'],
    action: 'set_priority',
    priority: 'P2',
  });
});

test('an incident is given a priority and a lower urgency from its menu, and the list shows them', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);

  const sent: Record<string, unknown> = {};
  const rated = { ...CHECKOUT, priority: 'P1' };
  const quiet = { ...DASHBOARD, urgency: 'low' };
  await page.route(/\/api\/v1\/alerts\/incidents(\?.*)?$/, (route) => route.fulfill({ json: [rated, quiet] }));
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-checkout$/, (route) => route.fulfill({ json: rated }));
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-checkout\/(priority|urgency)$/, (route) => {
    const action = new URL(route.request().url()).pathname.split('/').pop()!;
    sent[action] = route.request().postDataJSON();
    return route.fulfill({ json: rated });
  });

  // The list names the priority in its own column and says when an incident is low urgency.
  await page.goto('/alerts/incidents');
  await expect(page.locator('tbody tr').filter({ hasText: CHECKOUT.summary }).locator('[data-priority="P1"]')).toBeVisible();
  await expect(page.locator('tbody tr').filter({ hasText: DASHBOARD.summary })).toContainText('Low urgency');
  await page.getByRole('combobox', { name: 'Filter by priority' }).click();
  await page.getByRole('option', { name: 'P1' }).click();
  await expect(page.locator('tbody tr')).toHaveCount(1);

  await page.goto('/alerts/incidents/inc-checkout');
  const more = page.getByTestId('incident-more-actions');
  await more.click();
  await page.getByRole('menuitem', { name: 'Priority' }).click();
  await page.getByRole('menuitemradio', { name: 'P2' }).click();
  await expect.poll(() => sent.priority).toEqual({ priority: 'P2' });

  await expect(page.getByText('Priority changed', { exact: true })).toBeVisible();
  await page.mouse.move(400, 600);
  await expect(page.getByText('Priority changed', { exact: true })).toBeHidden();

  await more.click();
  await page.getByRole('menuitem', { name: 'Urgency' }).click();
  await page.getByRole('menuitemradio', { name: /Low urgency/ }).click();
  await expect.poll(() => sent.urgency).toEqual({ urgency: 'low' });
});

test('an escalation policy notifies again when an acknowledged incident stays unresolved', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);

  let saved: Record<string, unknown> | null = null;
  await page.route(/\/api\/v1\/alerts\/escalations\/esc-payments$/, (route) => {
    if (route.request().method() !== 'PUT') return route.fallback();
    saved = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ json: { id: 'esc-payments', orgId: 'acme-prod', ...saved } });
  });

  await page.goto('/alerts/escalations');
  await page.getByRole('row', { name: /Payments/ }).getByRole('button', { name: 'Edit' }).click();

  const drawer = page.getByRole('dialog');
  const retrigger = drawer.getByRole('combobox', { name: 'Acknowledged but unresolved' });
  await expect(retrigger).toContainText('Do not notify again');
  await retrigger.click();
  await page.getByRole('option', { name: '1 hour', exact: true }).click();
  await drawer.getByRole('button', { name: 'Save policy' }).click();
  await expect.poll(() => saved).toMatchObject({ name: 'Payments', ackRetriggerSecs: 3600 });
});

test('an escalation policy sets urgency by support hours and raises it when they begin', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);

  let saved: Record<string, unknown> | null = null;
  await page.route(/\/api\/v1\/alerts\/escalations\/esc-payments$/, (route) => {
    if (route.request().method() !== 'PUT') return route.fallback();
    saved = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ json: { id: 'esc-payments', orgId: 'acme-prod', ...saved } });
  });

  await page.goto('/alerts/escalations');
  await page.getByRole('row', { name: /Payments/ }).getByRole('button', { name: 'Edit' }).click();

  const drawer = page.getByRole('dialog');
  await expect(drawer.locator('[data-urgency-rule="by_severity"]')).toBeVisible();
  await drawer.getByText('By support hours', { exact: true }).click();
  await expect(drawer.locator('[data-weekly-windows]')).toBeVisible();
  await drawer.locator('[data-raise-at-start]').click();
  await drawer.getByRole('button', { name: 'Save policy' }).click();
  await expect.poll(() => saved).toMatchObject({
    name: 'Payments',
    urgencyRule: {
      mode: 'support_hours',
      raiseAtStart: true,
      hours: { ranges: [{ weekdayMask: 0b011_1110, start: '09:00', end: '18:00' }] },
    },
    escalateLowUrgency: false,
  });
});
