import type { Page } from '@playwright/test';

import { installApmRoutes, SERVICE_PATH } from '../fixtures/mockApm';
import { installApmSettingsRoutes } from '../fixtures/mockApmSettings';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

type Session = Parameters<typeof mountMockRoutes>[2];

async function boot(page: Page, port: number, options: { canary?: boolean; session?: Session } = {}) {
  await mountMockRoutes(page, port, options.session);
  await installApmRoutes(page, options);
  return installApmSettingsRoutes(page);
}

const card = (page: Page, name: string) => page.locator(`[data-apm-card="${name}"]`);

test('the deployment timeline puts what pipelines reported beside the versions spans carried', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(SERVICE_PATH.replace('?', '/deployments?'));

  const timeline = card(page, 'deployments');
  const deployed = timeline.locator('[data-apm-deployment="2.0.0"]');

  await expect(deployed).toHaveAttribute('data-kind', 'deployed');
  await expect(deployed).toContainText('Deployed');
  await expect(deployed.getByRole('link', { name: '9f2c1e7' })).toHaveAttribute('href', 'https://github.com/acme/checkout/commit/9f2c1e7a4b3d5e6f');
  await expect(deployed.getByRole('link', { name: 'PR' })).toHaveAttribute('href', 'https://github.com/acme/checkout/pull/42');
  await expect(deployed.getByRole('link', { name: 'CI run' })).toHaveAttribute('href', 'https://ci.example.com/runs/7');
  await expect(deployed).toContainText('ada');

  // A failed deployment is on the line, its version read on no row of its own.
  await expect(timeline.locator('[data-apm-deployment="1.9.9"]')).toHaveAttribute('data-kind', 'failed');
  await expect(timeline.locator('[data-apm-deployment="1.0.0"]')).toHaveAttribute('data-kind', 'first_seen');
  expect(await timeline.locator('[data-apm-deployment]').evaluateAll((rows) => rows.map((row) => row.getAttribute('data-apm-deployment')))).toEqual(['2.0.0', '1.9.9', '1.0.0']);

  // Only one version runs now: no canary.
  await expect(card(page, 'canary')).toHaveCount(0);
});

test('two versions running at once are read side by side, with how the traffic is shared', async ({ page, mockServer }) => {
  await boot(page, mockServer.port, { canary: true });
  await page.goto(SERVICE_PATH.replace('?', '/deployments?'));

  const canary = card(page, 'canary');
  await expect(canary).toContainText('2.0.0 running beside 1.0.0');
  await expect(canary.locator('[data-apm-canary-share]')).toHaveAttribute('data-apm-canary-share', '58');
  await expect(canary).toContainText('58.1%');
  await expect(canary).toContainText('Regressed');
});

test('collection quality suggests the settings that would read names as one, and each card saves its part', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto('/apm/settings');

  const quality = card(page, 'settings-quality');
  await expect(quality.locator('[data-apm-quality-finding="overflow"]')).toContainText('endpoints');
  await quality.locator('[data-apm-quality-finding="naming:checkout"]').getByRole('button', { name: 'Add a naming rule' }).click();

  const naming = card(page, 'settings-naming');
  await expect(naming.getByRole('textbox', { name: 'Pattern' })).toHaveValue('^checkout(-[a-z0-9]{5,}){1,2}$');
  await naming.getByRole('textbox', { name: 'Try a service name' }).fill('checkout-5f4d2c9b8-a1b2c');
  await expect(naming.locator('[data-apm-naming-result]')).toHaveText('Read as checkout');
  // The suggestion goes once it is a rule.
  await expect(quality.locator('[data-apm-quality-finding="naming:checkout"]')).toHaveCount(0);
  await naming.locator('[data-apm-settings-save="naming"]').click();
  await expect.poll(() => captured.settings.length).toBe(1);
  expect(captured.settings[0]).toMatchObject({ namingRules: [{ pattern: '^checkout(-[a-z0-9]{5,}){1,2}$', replacement: 'checkout' }] });

  await quality.locator('[data-apm-quality-finding="environment:production"]').getByRole('button', { name: 'Read all as production' }).click();
  await card(page, 'settings-environments').locator('[data-apm-settings-save="environments"]').click();
  await expect.poll(() => captured.settings.length).toBe(2);
  // Each save keeps what the others saved.
  expect(captured.settings[1]).toMatchObject({ environmentMap: [{ from: 'Production', to: 'production' }], namingRules: [{ replacement: 'checkout' }] });

  const thresholds = card(page, 'settings-thresholds');
  const warning = thresholds.getByRole('textbox', { name: 'Warning from' }).nth(1);
  await warning.fill('3000');
  await expect(thresholds).toContainText('The warning P95 is above 0 and below the critical one');
  await expect(thresholds.locator('[data-apm-settings-save="thresholds"]')).toBeDisabled();
  await warning.fill('300');
  await thresholds.locator('[data-apm-settings-save="thresholds"]').click();
  await expect.poll(() => captured.settings.length).toBe(3);
  expect(captured.settings[2]).toMatchObject({ health: { p95WarningTime: 300_000, p95CriticalTime: 2_000_000 } });
});

test('aliases are refused as the server would', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto('/apm/settings');

  const aliases = card(page, 'settings-aliases');
  await aliases.getByRole('button', { name: 'Add an alias' }).click();
  // A name field offers the services seen as it is typed.
  await aliases.getByRole('combobox', { name: 'Alias', exact: true }).fill('checkout');
  await aliases.getByRole('combobox', { name: 'Service', exact: true }).fill('checkout');
  await expect(aliases).toContainText('A service is not its own alias.');
  await aliases.getByRole('combobox', { name: 'Alias', exact: true }).fill('checkout-legacy');
  await aliases.locator('[data-apm-settings-save="aliases"]').click();
  await expect.poll(() => captured.settings.length).toBe(1);
  expect(captured.settings[0]).toMatchObject({ aliases: [{ alias: 'checkout-legacy', service: 'checkout' }] });

  // What the whole instance is limited to is not an organization's to set.
  await expect(card(page, 'settings-limits')).toHaveCount(0);
  await expect(card(page, 'settings-sampling')).toHaveCount(0);
});

test('the platform sets its limits and sampling here', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port, {
    session: {
      orgId: 'system-org-id',
      orgName: '_sys',
      role: 'Super administrator',
      scope: 'system',
      capabilityPermissions: [
        'sys.telemetry.read',
        'sys.telemetry.manage',
        'sys.settings.limits.read',
        'sys.settings.limits.manage',
      ],
    },
  });
  await page.goto('/apm/settings');

  const limits = card(page, 'settings-limits');
  await limits.getByRole('textbox', { name: 'Endpoints per service' }).fill('64');
  await limits.locator('[data-apm-settings-save="limits"]').click();
  await expect.poll(() => captured.limits.length).toBe(1);
  expect(captured.limits[0]).toMatchObject({ transactionsPerServiceHour: 64, servicesPerOrgHour: 200 });

  const sampling = card(page, 'settings-sampling');
  await expect(sampling).toContainText('version 3');
  await expect(sampling.getByRole('textbox', { name: 'Name' })).toHaveValue('Keep checkout errors');
  await sampling.getByRole('textbox', { name: 'Normal traces kept' }).fill('20');
  await sampling.locator('[data-apm-settings-save="sampling"]').click();
  await expect.poll(() => captured.policies.length).toBe(1);
  expect(captured.policies[0]).toMatchObject({ normalSampleRatio: 0.2, rules: [{ name: 'Keep checkout errors', requireError: true, action: { type: 'keep' } }] });
  await expect(sampling).toContainText('version 4');
});

test('an endpoint is a long task from the threshold the organization set', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.route(/\/api\/v1\/apm\/settings$/, (route) =>
    route.fulfill({ json: { health: { errorRateWarning: 0.02, errorRateCritical: 0.1, p95WarningTime: 500_000, p95CriticalTime: 2_000_000 }, apdexThreshold: 500_000, longTaskTime: 10_000_000, namingRules: [], environmentMap: [], aliases: [], metadataRules: { teamFromTier: null, ownerFromTier: null, repositoryFromTier: null } } }),
  );
  await page.goto(SERVICE_PATH.replace('?', '/endpoints?'));

  await expect(page.locator('[data-apm-endpoint-row="POST /orders"]')).toBeVisible();
  // Ten seconds is past every endpoint's P99: none is a long task.
  await expect(page.locator('[data-apm-endpoints-long-tasks]')).toHaveCount(0);
});
