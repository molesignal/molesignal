/**
 * People in escalation policies and incidents when the member list is late, leaves them out, says
 * no more than their names, or cannot be read. A real node takes seconds over that list, so a policy
 * can be opened and an incident read before it arrives; the mock answers at once unless a test holds
 * it back, and answers every role in full unless a test says otherwise.
 */
import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.now() * 1000;

const POLICY = {
  id: 'esc-weekday',
  orgId: 'acme-prod',
  name: 'Weekday on-call',
  steps: [{ targets: [{ kind: 'user', userId: 'u1' }], ackTimeoutSecs: 300, minSeverity: null }],
  repeat: false,
  maxLoops: 1,
  ackRetriggerSecs: null,
  urgencyRule: { mode: 'by_severity' },
  escalateLowUrgency: false,
};

function incident(createdBy: string, assignees: string[]) {
  return {
    id: 'inc-hand',
    orgId: 'acme-prod',
    ruleId: 'manual',
    escalationPolicyId: 'default',
    status: 'open',
    severity: 'critical',
    summary: 'Customers in the north cannot pay',
    fingerprint: 'fp-hand',
    currentStep: 0,
    currentLoop: 0,
    currentStepStartedAt: NOW,
    assignees,
    labels: { 'alert.source': 'manual' },
    annotations: { created_by: createdBy },
    traceIds: [],
    hostIds: [],
    affectedServices: [],
    triggeringQuery: null,
    createdAt: NOW,
    urgency: 'high',
  };
}

/** Holds the member list back until the returned function is called. */
async function holdMemberList(page: Page) {
  let release = () => {};
  const held = new Promise<void>((resolve) => {
    release = resolve;
  });
  await page.route(/\/api\/v1\/users$/, async (route) => {
    await held;
    return route.fallback();
  });
  return release;
}

async function servePolicy(page: Page) {
  const saved: Array<Record<string, unknown>> = [];
  await page.route(/\/api\/v1\/alerts\/escalations$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: [POLICY] }) : route.fallback(),
  );
  await page.route(/\/api\/v1\/alerts\/escalations\/esc-weekday$/, (route) => {
    if (route.request().method() !== 'PUT') return route.fallback();
    const body = route.request().postDataJSON() as Record<string, unknown>;
    saved.push(body);
    return route.fulfill({ json: { ...POLICY, ...body } });
  });
  return saved;
}

/** What the server tells someone without `org.members.read`. */
const serveNamesOnly = (page: Page) =>
  page.route(/\/api\/v1\/users$/, (route) =>
    route.fulfill({
      json: [
        { id: 'u1', displayName: 'Dana Reyes', avatarUrl: null },
        { id: 'u2', displayName: 'Kenji Watanabe', avatarUrl: null },
      ],
    }),
  );

const serveIncident = (page: Page, createdBy: string, assignees: string[]) =>
  page.route(/\/api\/v1\/alerts\/incidents\/inc-hand$/, (route) =>
    route.fulfill({ json: incident(createdBy, assignees) }),
  );

const targetKept = { steps: [{ targets: [{ kind: 'user', userId: 'u1' }] }] };

test('a policy opened before the member list arrives keeps its person', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const release = await holdMemberList(page);
  const saved = await servePolicy(page);

  await page.goto('/alerts/escalations');
  const row = page.getByRole('row', { name: /Weekday on-call/ });
  await expect(row).toContainText('…');
  await expect(row).not.toContainText('u1');
  await row.getByRole('button', { name: 'Edit' }).click();

  const drawer = page.getByRole('dialog');
  await expect(drawer.getByRole('combobox').filter({ hasText: '…' })).toHaveCount(1);

  release();

  await expect(drawer.getByRole('combobox').filter({ hasText: 'Dana Reyes' })).toHaveCount(1);
  await expect(drawer.getByRole('combobox').filter({ hasText: '— select —' })).toHaveCount(0);

  await drawer.getByRole('button', { name: 'Save policy' }).click();
  await expect.poll(() => saved[0]).toMatchObject(targetKept);
});

test('a policy saved before the member list arrives keeps its person', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await holdMemberList(page);
  const saved = await servePolicy(page);

  await page.goto('/alerts/escalations');
  await page.getByRole('row', { name: /Weekday on-call/ }).getByRole('button', { name: 'Edit' }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Save policy' }).click();

  await expect.poll(() => saved[0]).toMatchObject(targetKept);
});

test('someone is picked from the member list in a step', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const saved = await servePolicy(page);

  await page.goto('/alerts/escalations');
  await page.getByRole('row', { name: /Weekday on-call/ }).getByRole('button', { name: 'Edit' }).click();

  const drawer = page.getByRole('dialog');
  await drawer.getByRole('combobox').filter({ hasText: 'Dana Reyes' }).click();
  await page.getByRole('option', { name: 'Kenji Watanabe' }).click();
  await drawer.getByRole('button', { name: 'Save policy' }).click();

  await expect.poll(() => saved[0]).toMatchObject({
    steps: [{ targets: [{ kind: 'user', userId: 'u2' }] }],
  });
});

test('a policy that names someone outside the member list says so and keeps them', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const saved = await servePolicy(page);
  await page.route(/\/api\/v1\/users$/, (route) => route.fulfill({ json: [] }));

  await page.goto('/alerts/escalations');
  await page.getByRole('row', { name: /Weekday on-call/ }).getByRole('button', { name: 'Edit' }).click();

  const drawer = page.getByRole('dialog');
  await expect(drawer.getByRole('combobox').filter({ hasText: 'Non-member' })).toHaveCount(1);

  await drawer.getByRole('button', { name: 'Save policy' }).click();
  await expect.poll(() => saved[0]).toMatchObject(targetKept);
});

test('a policy keeps its people when the member list cannot be read', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const saved = await servePolicy(page);
  await page.route(/\/api\/v1\/users$/, (route) =>
    route.fulfill({ status: 500, json: { error: 'internal', message: 'internal error' } }),
  );

  await page.goto('/alerts/escalations');
  await page.getByRole('row', { name: /Weekday on-call/ }).getByRole('button', { name: 'Edit' }).click();

  const drawer = page.getByRole('dialog');
  await expect(drawer.getByRole('combobox').filter({ hasText: 'Someone' })).toHaveCount(1);

  await drawer.getByRole('button', { name: 'Save policy' }).click();
  await expect.poll(() => saved[0]).toMatchObject(targetKept);
});

test('an incident names who opened it and who has it, once the member list arrives', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const release = await holdMemberList(page);
  await serveIncident(page, 'u2', ['u1']);

  await page.goto('/alerts/incidents/inc-hand');
  const openedBy = page.locator('[data-incident-created-by]');
  await expect(openedBy).toHaveText('…');

  release();

  await expect(openedBy).toHaveText('Kenji Watanabe');
  const body = page.locator('main');
  await expect(body.getByText('Dana Reyes').first()).toBeVisible();
  await expect(body).not.toContainText('u2');
});

test('an incident opened by whoever is signed in names them even outside the member list', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await serveIncident(page, 'dev', ['dev']);

  await page.goto('/alerts/incidents/inc-hand');

  await expect(page.locator('[data-incident-created-by]')).toHaveText('Dev User');
});

test('an incident opened by someone outside the member list says so', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await serveIncident(page, 'root-id', ['root-id']);

  await page.goto('/alerts/incidents/inc-hand');

  await expect(page.locator('[data-incident-created-by]')).toHaveText('Non-member');
  await expect(page.locator('main')).not.toContainText('root-id');
});

test('a role that may not read members picks people for a step by name', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port, { role: 'Editor' });
  await serveNamesOnly(page);
  const saved = await servePolicy(page);

  await page.goto('/alerts/escalations');
  const row = page.getByRole('row', { name: /Weekday on-call/ });
  await expect(row).toContainText('Dana Reyes');
  await row.getByRole('button', { name: 'Edit' }).click();

  const drawer = page.getByRole('dialog');
  await drawer.getByRole('combobox').filter({ hasText: 'Dana Reyes' }).click();
  await page.getByRole('option', { name: 'Kenji Watanabe' }).click();
  await drawer.getByRole('button', { name: 'Save policy' }).click();

  await expect.poll(() => saved[0]).toMatchObject({
    steps: [{ targets: [{ kind: 'user', userId: 'u2' }] }],
  });
});

test('a role that may not read members reads an incident by name', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port, { role: 'Viewer' });
  await serveNamesOnly(page);
  await serveIncident(page, 'u2', ['u1']);

  await page.goto('/alerts/incidents/inc-hand');

  await expect(page.locator('[data-incident-created-by]')).toHaveText('Kenji Watanabe');
  await expect(page.locator('main').getByText('Dana Reyes').first()).toBeVisible();
  await expect(page.locator('main')).not.toContainText('u2');
});

test('a role that may not read members reassigns to someone found by name', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port, { role: 'Editor' });
  await serveNamesOnly(page);
  await serveIncident(page, 'u1', ['u1']);
  let sent: unknown = null;
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-hand\/reassign$/, (route) => {
    sent = route.request().postDataJSON();
    return route.fulfill({ json: incident('u1', ['u2']) });
  });

  await page.goto('/alerts/incidents/inc-hand');
  await page.getByRole('button', { name: 'More actions' }).click();
  await page.getByRole('menuitem', { name: 'Reassign' }).click();

  const dialog = page.getByRole('dialog', { name: 'Reassign' });
  await dialog.getByRole('textbox', { name: 'Search by name or email' }).fill('kenji');
  await expect(dialog.getByRole('checkbox', { name: 'Dana Reyes' })).toHaveCount(0);
  await dialog.getByRole('checkbox', { name: 'Kenji Watanabe' }).check();
  await dialog.getByTestId('incident-assign-submit').click();

  await expect.poll(() => sent).toEqual({ kind: 'users', userIds: ['u2'] });
});
