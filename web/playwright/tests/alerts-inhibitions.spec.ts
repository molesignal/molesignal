/**
 * Inhibition: a rule holds back the notifications of the lesser incidents while a worse one is active.
 * The rule is written and tried on its own page, the incidents it holds back say so in the list and on
 * their page, and a person can let one notify anyway. The firing rule says it is held back too.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.now() * 1000;
const MINUTE = 60_000_000;

const rule = {
  id: 'inhibit-1',
  orgId: 'acme-prod',
  name: 'Same service',
  enabled: true,
  source: { matchers: [], minSeverity: 'critical' },
  target: { matchers: [{ label: 'env', op: 'eq', value: 'prod' }], maxSeverity: 'warning' },
  equal: ['service'],
  moreSevereOnly: true,
  waitSecs: 60,
  lingerSecs: 120,
  comment: 'The database going down takes everything with it.',
  createdBy: 'u1',
  createdAt: NOW - 60 * MINUTE,
  updatedAt: NOW - 60 * MINUTE,
  inhibiting: 1,
};

const incident = (id: string, summary: string, fields: Record<string, unknown>) => ({
  id,
  orgId: 'acme-prod',
  ruleId: 'rule-checkout-latency',
  escalationPolicyId: 'esc-payments',
  status: 'open',
  severity: 'warning',
  summary,
  fingerprint: `fp-${id}`,
  currentStep: 0,
  currentLoop: 0,
  currentStepStartedAt: NOW - 10 * MINUTE,
  assignees: [],
  labels: { service: 'checkout', env: 'prod' },
  annotations: {},
  traceIds: [],
  hostIds: [],
  affectedServices: [],
  triggeringQuery: null,
  createdAt: NOW - 10 * MINUTE,
  ...fields,
});

const WORSE = incident('inc-worse', 'Checkout database is down', { severity: 'critical', ruleId: 'rule-other' });
const HELD = incident('inc-held', 'Checkout p95 above 800 ms', {
  inhibition: {
    ruleId: 'inhibit-1',
    ruleName: 'Same service',
    sourceIncidentId: 'inc-worse',
    sourceSummary: 'Checkout database is down',
    since: NOW - 5 * MINUTE,
  },
});

test('a rule is written, tried on the incidents of now, and says what it holds back', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const saved: Array<Record<string, unknown>> = [];
  let previewed: Record<string, unknown> | null = null;
  let rules = [rule];

  await page.route(/\/api\/v1\/alerts\/inhibit-rules$/, async (route) => {
    if (route.request().method() === 'POST') {
      const body = route.request().postDataJSON() as Record<string, unknown>;
      saved.push(body);
      rules = [...rules, { ...rule, ...body, id: 'inhibit-2', inhibiting: 0 } as typeof rule];
      return route.fulfill({ json: rules.at(-1) });
    }

    return route.fulfill({ json: rules });
  });

  await page.route(/\/api\/v1\/alerts\/inhibit-rules\/preview$/, (route) => {
    previewed = route.request().postDataJSON() as Record<string, unknown>;

    return route.fulfill({
      json: {
        pairs: [
          {
            sourceIncidentId: 'inc-worse',
            sourceSummary: 'Checkout database is down',
            sourceSeverity: 'critical',
            targetIncidentId: 'inc-held',
            targetSummary: 'Checkout p95 above 800 ms',
            targetSeverity: 'warning',
          },
        ],
      },
    });
  });

  await page.goto('/alerts/inhibitions');

  // A row reads as the sentence it is, and says what it is doing now.
  const row = page.getByRole('row', { name: /Same service/ });
  await expect(row).toContainText('At least Critical');
  await expect(row).toContainText('env');
  await expect(row).toContainText('service');
  await expect(row).toContainText('Holding back 1 incident');

  await page.getByRole('button', { name: 'New inhibit rule' }).click();

  const drawer = page.locator('[data-inhibit-rule-form]');
  await drawer.getByRole('textbox', { name: /^Name/ }).fill('Database first');

  // Without a source and without shared labels any incident would hold back every other.
  const shared = drawer.getByRole('textbox', { name: /These labels must match/ });
  await expect(shared).toHaveValue('service');
  await shared.fill('');
  await expect(page.getByRole('button', { name: 'Create rule' })).toBeDisabled();
  await expect(drawer.getByRole('alert')).toContainText('any incident would hold back every other');
  await shared.fill('service, env');

  await drawer.locator('[data-inhibit-preview-run]').click();
  await expect(drawer.locator('[data-inhibit-preview]')).toContainText('Checkout p95 above 800 ms');
  await expect(drawer.locator('[data-inhibit-preview]')).toContainText('held back by');
  await expect(drawer.locator('[data-inhibit-preview]')).toContainText('Checkout database is down');
  expect(previewed).toMatchObject({ equal: ['service', 'env'], moreSevereOnly: true, waitSecs: 60, lingerSecs: 120 });

  // Changing the rule takes back what was previewed: it no longer says what this rule does.
  await drawer.getByRole('checkbox', { name: 'The source must be worse than the target' }).click();
  await expect(drawer.locator('[data-inhibit-preview]')).toHaveCount(0);

  await page.getByRole('button', { name: 'Create rule' }).click();
  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({ name: 'Database first', equal: ['service', 'env'], moreSevereOnly: false });
});

test('deleting a rule that holds incidents back says they will notify', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let deleted = false;
  await page.route(/\/api\/v1\/alerts\/inhibit-rules$/, (route) => route.fulfill({ json: deleted ? [] : [rule] }));
  await page.route(/\/api\/v1\/alerts\/inhibit-rules\/inhibit-1$/, (route) => {
    deleted = true;
    return route.fulfill({ json: { deleted: true } });
  });

  await page.goto('/alerts/inhibitions');
  await page.getByRole('button', { name: 'Delete inhibit rule Same service' }).click();

  const dialog = page.getByRole('alertdialog').or(page.getByRole('dialog'));
  await expect(dialog).toContainText('The 1 incident it holds back notifies at once');
  await dialog.getByRole('button', { name: 'Delete' }).click();

  await expect(page.getByText('No inhibit rules')).toBeVisible();
});

test('an inhibited incident says why in the list and on its page, and can notify anyway', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);

  let lifted = false;
  const current = () => (lifted ? { ...HELD, inhibition: null, inhibitionLiftedBy: 'u1' } : HELD);

  await page.route(/\/api\/v1\/alerts\/incidents(\?.*)?$/, (route) => route.fulfill({ json: [WORSE, current()] }));
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-held$/, (route) => route.fulfill({ json: current() }));
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-held\/timeline$/, (route) =>
    route.fulfill({
      json: {
        entries: [
          {
            id: 'tl-1',
            orgId: 'acme-prod',
            incidentId: 'inc-held',
            at: NOW - 5 * MINUTE,
            kind: 'inhibited',
            actor: null,
            detail: {
              ruleId: 'inhibit-1',
              ruleName: 'Same service',
              sourceIncidentId: 'inc-worse',
              sourceSummary: 'Checkout database is down',
            },
          },
        ],
        deliveries: [],
      },
    }),
  );
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-held\/inhibition\/lift$/, (route) => {
    lifted = true;
    return route.fulfill({ json: current() });
  });

  await page.goto('/alerts/incidents');

  const heldRow = page.getByRole('row', { name: /Checkout p95 above 800 ms/ });
  await expect(heldRow).toContainText('Inhibited');
  await expect(page.getByRole('row', { name: /Checkout database is down/ })).not.toContainText('Inhibited');

  // The view that lists only what is held back is offered while something is.
  await page.locator('[data-alert-filter-tabs]').getByRole('button', { name: /Inhibited/ }).click();
  await expect(page.getByRole('row', { name: /Checkout database is down/ })).toHaveCount(0);
  await expect(heldRow).toBeVisible();

  await page.goto('/alerts/incidents/inc-held');

  const note = page.locator('[data-incident-inhibition="held"]');
  await expect(note).toContainText('Notifications are held back because of');
  await expect(note).toContainText('Checkout database is down');
  await expect(note).toContainText('Same service');
  await expect(page.getByText('Inhibited by rule Same service')).toBeVisible();

  await note.locator('[data-incident-lift-inhibition]').click();
  await expect.poll(() => lifted).toBe(true);
  await expect(note).toHaveCount(0);
});

test('a rule whose every incident is held back says so', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  await page.route(/\/api\/v1\/alerts\/incidents(\?.*)?$/, (route) => route.fulfill({ json: [HELD] }));

  await page.goto('/alerts/rules');

  await expect(page.getByRole('row', { name: /Checkout latency|checkout/i }).first()).toContainText('Firing (inhibited)');
});
