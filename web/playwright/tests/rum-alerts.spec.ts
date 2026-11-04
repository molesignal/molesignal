import AxeBuilder from '@axe-core/playwright';
import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { installRumAlertCenter } from '../fixtures/mockRumAlerts';
import { MOCK_RUM } from '../fixtures/mockRumData';

const ALERTS = '[data-rum-alerts-drawer]';
const WEB = MOCK_RUM.applications.web;
const APP = MOCK_RUM.applications.app;

async function boot(page: Page, port: number, options: Parameters<typeof mountMockRoutes>[2] = {}) {
  await mountMockRoutes(page, port, options);

  return installRumAlertCenter(page);
}

/** Waits until what slides in has stopped: a drawer that is still moving is scanned half transparent. The animations that never end (a pulse, a spinner) are not waited for. */
async function settled(page: Page) {
  await page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        .filter((animation) => Number.isFinite(animation.effect?.getComputedTiming().iterations ?? 1))
        .map((animation) => animation.finished),
    ),
  );
}

/** Opens the drawer of the alerts of the page and waits until the rules are read. */
async function openAlerts(page: Page) {
  await page.locator('[data-rum-alerts-button]').click();
  await expect(page.locator(ALERTS)).toBeVisible();
  await expect(page.locator(`${ALERTS} [data-rum-alert-template]`).first()).toBeVisible();
}

test.describe('RUM alerts', () => {
  test('are at the end of the row a page is scoped by, with the number of incidents going on in the colour of the worst', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(`/rum/overview?app=${WEB}&env=prod`);

    const button = page.locator('[data-rum-alerts-button]');

    await expect(button).toBeVisible();
    // The incident of another application is not counted.
    await expect(button).toHaveAttribute('data-rum-alerts-count', '1');
    await expect(button).toContainText('Alerts');
    await expect(button).toContainText('1');
    await expect(button).toHaveAttribute('aria-label', 'Alerts, 1 going on');
    await expect(button.locator('span')).toHaveClass(/red-ink/);
  });

  // Every page of the application has it, in the same place.
  for (const path of ['errors', 'performance', 'frustration']) {
    test(`stand in the scope row of the ${path} page too`, async ({ page, mockServer }) => {
      await boot(page, mockServer.port);
      await page.goto(`/rum/${path}?app=${WEB}&env=prod`);

      await expect(page.locator('[data-rum-scope-row] [data-rum-alerts-button]')).toBeVisible();
      await expect(page.locator('[data-rum-alerts-button]')).toHaveAttribute('data-rum-alerts-count', '1');
    });
  }

  test('say what is going on, which rules watch the application, and the templates, in that order', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(`/rum/overview?app=${WEB}&env=prod`);
    await openAlerts(page);

    await expect(page.locator('[data-rum-alerts-scope]')).toContainText('Storefront · prod');

    const order = await page.locator(`${ALERTS} [data-rum-panel-section]`).evaluateAll((sections) => sections.map((section) => section.getAttribute('data-rum-panel-section')));

    expect(order).toEqual(['alerts-going-on', 'alerts-rules', 'alerts-templates']);

    // What is going on: the incident of this application, how bad, which rule, the line, when.
    const incident = page.locator('[data-rum-alert-incident="inc-rum-lcp"]');

    await expect(page.locator('[data-rum-alert-incident]')).toHaveCount(1);
    await expect(incident).toContainText('Critical');
    await expect(incident).toContainText('Storefront · /checkout page loads slowly');
    await expect(incident).toContainText('LCP (p75) · > 4.0 s · 12 minutes ago');

    // The rules: those of the environment in front and those of every environment, those that fire first.
    const rules = page.locator('[data-rum-alert-rule]');

    await expect(rules).toHaveCount(3);
    await expect(rules.nth(0)).toHaveAttribute('data-rum-alert-rule', 'rule-rum-lcp');
    await expect(rules.nth(0).locator('[data-rum-alert-status]')).toHaveText('Firing');
    await expect(rules.nth(1).locator('[data-rum-alert-status]')).toHaveText('Healthy');
    await expect(rules.nth(2).locator('[data-rum-alert-status]')).toHaveText('Off');
    // Not the rule of staging, and not the rule of another application.
    await expect(page.locator('[data-rum-alert-rule="rule-rum-staging-cls"]')).toHaveCount(0);
    await expect(page.locator('[data-rum-alert-rule="rule-rum-other"]')).toHaveCount(0);
    await expect(page.locator('[data-rum-alert-new-rule]')).toHaveAttribute('href', `/alerts/rules/new?source=rum&app=${WEB}&metric=error_rate&environment=prod`);
    await expect(rules.nth(0).getByRole('link', { name: 'Edit' })).toHaveAttribute('href', '/alerts/rules/rule-rum-lcp/edit');
  });

  test('follow the environment of the page: staging has its own rule, and the incident of production is not its own', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(`/rum/overview?app=${WEB}&env=staging`);
    await openAlerts(page);

    await expect(page.locator('[data-rum-alerts-scope]')).toContainText('Storefront · staging');
    // The incident is production's, and says so by its label.
    await expect(page.locator('[data-rum-alert-incident]')).toHaveCount(0);
    await expect(page.locator('[data-rum-alert-rule]')).toHaveCount(2);
    await expect(page.locator('[data-rum-alert-rule="rule-rum-staging-cls"]')).toBeVisible();
    await expect(page.locator('[data-rum-alert-rule="rule-rum-frustration"]')).toBeVisible();
  });

  test('are of every environment when the page is: all the incidents and all the rules of the application', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(`/rum/overview?app=${WEB}`);
    await openAlerts(page);

    await expect(page.locator('[data-rum-alerts-scope]')).not.toContainText('prod');
    await expect(page.locator('[data-rum-alert-incident]')).toHaveCount(1);
    await expect(page.locator('[data-rum-alert-rule]')).toHaveCount(4);
  });

  test('list ten templates for a browser application with the starting set first, each with the line it pages on and the figure now', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(`/rum/overview?app=${WEB}&env=prod`);
    await openAlerts(page);

    const templates = page.locator('[data-rum-alert-template]');

    await expect(templates).toHaveCount(10);
    expect(await templates.evaluateAll((items) => items.map((item) => item.getAttribute('data-rum-alert-template')))).toEqual([
      'lcp',
      'inp',
      'error_rate',
      'failed_requests',
      'new_issues',
      'cls',
      'frustration',
      'regressions',
      'high_impact',
      'traffic',
    ]);

    const lcp = page.locator('[data-rum-alert-template="lcp"]');

    await expect(lcp).toContainText('Slow page loads');
    await expect(lcp).toContainText('LCP (p75)');
    await expect(lcp).toContainText('Recommended');
    await expect(lcp).toContainText('Warning > 2.5 s, Critical > 4.0 s · 1 hour · at least 50 views that have a value');
    await expect(lcp.locator('[data-rum-alert-now]')).toContainText('Now 2.4 s Good');

    // A figure that is over its line already says what the rule would do at once.
    await expect(page.locator('[data-rum-alert-template="cls"] [data-rum-alert-now]')).toContainText('Now 0.13 Needs improvement');
    await expect(page.locator('[data-rum-alert-template="cls"] [data-rum-alert-now]')).toContainText('It would be in Warning as soon as it is created');
    await expect(page.locator('[data-rum-alert-template="lcp"] [data-rum-alert-now]')).not.toContainText('as soon as it is created');

    // A figure with too few rows has no value, and says what it waits for.
    await expect(page.locator('[data-rum-alert-template="frustration"] [data-rum-alert-no-value]')).toContainText('No value yet: fewer than 100 sessions in 1 hour');
    // Counts are told by a sentence, and the traffic one is optional.
    await expect(page.locator('[data-rum-alert-template="new_issues"]')).toContainText('More than 0 issues first seen in prod in the window → Warning · 15 minutes');
    await expect(page.locator('[data-rum-alert-template="traffic"]')).toContainText('Optional');
    await expect(page.locator('[data-rum-alert-template="traffic"] [data-rum-alert-now]')).toContainText('Now 38 sessions in 15 minutes');
  });

  test('know what was made from a template for this environment, and do not offer it again', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(`/rum/overview?app=${WEB}&env=prod`);
    await openAlerts(page);

    const made = page.locator('[data-rum-alert-template="error_rate"]');

    await expect(made).toHaveAttribute('data-rum-alert-created', '');
    await expect(made).toContainText('Created and enabled');
    await expect(made.getByRole('link', { name: 'View rule' })).toHaveAttribute('href', '/alerts/rules/rule-rum-errors/edit');
    await expect(made.locator('[data-rum-alert-create]')).toHaveCount(0);
    // The one of every environment is not the one of production.
    await expect(page.locator('[data-rum-alert-template="frustration"]')).not.toHaveAttribute('data-rum-alert-created', '');
  });

  test('make an enabled rule with two lines from a template in one click, and show it made', async ({ page, mockServer }) => {
    const center = await boot(page, mockServer.port);

    await page.goto(`/rum/overview?app=${WEB}&env=prod`);
    await openAlerts(page);
    await page.locator('[data-rum-alert-create="inp"]').click();

    await expect.poll(() => center.rules.length).toBe(1);
    expect(center.rules[0]).toMatchObject({
      name: 'Storefront · prod · Slow interactions',
      enabled: true,
      kind: 'scheduled',
      query: { stream: null, periodSecs: 3_600, rum: { applicationId: WEB, environment: 'prod', metric: 'inp_p75' } },
      trigger: { operator: 'gt', threshold: 200, forPeriods: 1 },
      thresholds: [
        { severity: 'warning', operator: 'gt', threshold: 200 },
        { severity: 'critical', operator: 'gt', threshold: 500 },
      ],
      labels: { 'rum.application': WEB, 'deployment.environment': 'prod', 'rum.template': 'inp' },
    });
    // The default escalation policy is the server's.
    expect(center.rules[0]).not.toHaveProperty('escalationPolicyId');

    const made = page.locator('[data-rum-alert-template="inp"]');

    await expect(made).toHaveAttribute('data-rum-alert-created', '');
    await expect(made).toContainText('Created and enabled');
    // And the rule is listed with the rules of the application.
    await expect(page.locator('[data-rum-alert-rule]')).toHaveCount(4);
    await expect(page.locator('[data-rum-alert-rule]', { hasText: 'Storefront · prod · Slow interactions' })).toBeVisible();
  });

  test('make the rule for every environment when the page is of all of them', async ({ page, mockServer }) => {
    const center = await boot(page, mockServer.port);

    await page.goto(`/rum/overview?app=${WEB}`);
    await openAlerts(page);
    await page.locator('[data-rum-alert-create="traffic"]').click();

    await expect.poll(() => center.rules.length).toBe(1);
    expect(center.rules[0]).toMatchObject({
      name: 'Storefront · Traffic stopped',
      query: { periodSecs: 900, rum: { applicationId: WEB, environment: null, metric: 'session_count' } },
      thresholds: [{ severity: 'critical', operator: 'lt', threshold: 1 }],
    });
    expect((center.rules[0] as { labels: Record<string, string> }).labels).not.toHaveProperty('deployment.environment');
  });

  test('offer eight to an app, with crashes where a browser has vitals', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(`/rum/overview?app=${APP}`);
    await openAlerts(page);

    const templates = page.locator('[data-rum-alert-template]');

    await expect(templates).toHaveCount(8);
    expect(await templates.evaluateAll((items) => items.map((item) => item.getAttribute('data-rum-alert-template')))).toEqual([
      'crash_free',
      'error_rate',
      'failed_requests',
      'new_issues',
      'frustration',
      'regressions',
      'high_impact',
      'traffic',
    ]);
    await expect(page.locator('[data-rum-alert-template="crash_free"]')).toContainText('Warning < 99.5%, Critical < 99%');
    await expect(page.locator('[data-rum-alert-template="error_rate"]')).toContainText('Screen errors');
  });

  test('keep a person who may read alerts but not manage them from making rules, and say why', async ({ page, mockServer }) => {
    await boot(page, mockServer.port, { role: 'viewer', capabilityPermissions: ['rum.events.read', 'alerts.rules.read'] });
    await page.goto(`/rum/overview?app=${WEB}&env=prod`);
    await openAlerts(page);

    await expect(page.locator('[data-rum-alert-create="inp"]')).toBeDisabled();
    await expect(page.locator('[data-rum-alert-new-rule]')).toHaveCount(0);
    await expect(page.locator('[data-rum-alert-rule]').first().getByRole('link', { name: 'Edit' })).toHaveCount(0);
    // What exists is still told.
    await expect(page.locator('[data-rum-alert-template="error_rate"]')).toContainText('Created and enabled');
  });

  test('are not shown to a person who may not read alerts', async ({ page, mockServer }) => {
    await boot(page, mockServer.port, { role: 'viewer', capabilityPermissions: ['rum.events.read'] });
    await page.goto(`/rum/overview?app=${WEB}&env=prod`);

    await expect(page.locator('[data-rum-scope="application"]')).toBeVisible();
    await expect(page.locator('[data-rum-alerts-button]')).toHaveCount(0);
  });

  test('open an incident over the page, with the sessions that tell it and a way to the page of RUM that shows its figure', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(`/rum/overview?app=${WEB}&env=prod`);
    await openAlerts(page);
    await page.locator('[data-rum-alert-incident="inc-rum-lcp"]').getByRole('button', { name: 'Open' }).click();

    // The drawer of the alerts gives way to the one of the incident.
    await expect(page.locator(ALERTS)).toHaveCount(0);

    const sessions = page.locator('[data-incident-rum-sessions]');

    await expect(sessions).toBeVisible();
    await expect(page.getByText('Sessions of the views with the slowest largest contentful paint')).toBeVisible();

    const rows = sessions.locator('[data-incident-rum-session]');

    await expect(rows).toHaveCount(3);
    await expect(rows.nth(0)).toHaveAttribute('data-incident-rum-session', MOCK_RUM.session.checkout);
    await expect(rows.nth(0)).toContainText('LCP 7.2 s');
    await expect(rows.nth(0)).toContainText('element img.hero');
    await expect(rows.nth(1)).toContainText('LCP 6.4 s');
    await expect(rows.nth(1)).toContainText('element div.order-summary');
    await expect(rows.nth(2)).toContainText('LCP 5.8 s');

    // Who they were comes from the sessions, and a recorded one opens at its replay.
    const replay = rows.nth(0).getByRole('link');

    await expect(replay).toHaveAttribute('data-incident-rum-replay', '');
    await expect(replay).toContainText('Replay');

    const url = new URL((await replay.getAttribute('href'))!, 'https://x.test');

    expect(url.pathname).toBe(`/rum/sessions/view/${MOCK_RUM.session.checkout}`);
    expect(Number(url.searchParams.get('at'))).toBeGreaterThan(0);

    // The way on: the page that tells the figure, over the time of the incident.
    const open = page.getByRole('link', { name: /Open in RUM/ });
    const target = new URL((await open.getAttribute('href'))!, 'https://x.test');

    expect(target.pathname).toBe('/rum/performance');
    expect(target.searchParams.get('app')).toBe(WEB);
    expect(target.searchParams.get('env')).toBe('prod');
    expect(target.searchParams.get('vital')).toBe('lcp');
    expect(target.searchParams.get('open')).toBe('/checkout');
    expect(target.searchParams.get('time')).toContain('..');
    // The statement of a RUM rule only names its figure, so there is no query to open.
    await expect(page.getByRole('link', { name: 'Open triggering query' })).toHaveCount(0);
  });

  for (const theme of ['dark', 'light'] as const) {
    test(`have nothing serious or critical for axe in the drawer and in the sample sessions, ${theme} theme`, async ({ page, mockServer }) => {
      await boot(page, mockServer.port, { theme });
      await page.goto(`/rum/overview?app=${WEB}&env=prod`);
      await openAlerts(page);
      await expect(page.locator('[data-rum-alert-template="traffic"] [data-rum-alert-now]')).toContainText('Now');
      await settled(page);

      const scan = async (selector: string) => {
        const results = await new AxeBuilder({ page }).include(selector).exclude('[role="status"]').analyze();

        return results.violations.filter((violation) => violation.impact === 'critical' || violation.impact === 'serious');
      };

      expect(await scan(ALERTS)).toEqual([]);

      await page.locator('[data-rum-alert-incident="inc-rum-lcp"]').getByRole('button', { name: 'Open' }).click();
      await expect(page.locator('[data-incident-rum-session]')).toHaveCount(3);
      await settled(page);

      expect(await scan('[data-incident-rum-sessions]')).toEqual([]);
    });
  }

  test('offer to alert on a page or a version from the drawer of its row on the performance page, but not on a browser', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(`/rum/performance?app=${WEB}&env=prod&open=%2Fcheckout&tab=inp`);

    const link = page.locator('[data-rum-attribution-alert]');

    await expect(link).toBeVisible();
    await expect(link).toContainText('Alert on this');

    const target = new URL((await link.getAttribute('href'))!, 'https://x.test');

    expect(target.pathname).toBe('/alerts/rules/new');
    expect(target.searchParams.get('source')).toBe('rum');
    expect(target.searchParams.get('app')).toBe(WEB);
    expect(target.searchParams.get('environment')).toBe('prod');
    expect(target.searchParams.get('page')).toBe('/checkout');
    // The vital in front is the figure.
    expect(target.searchParams.get('metric')).toBe('inp_p75');

    await page.goto(`/rum/performance?app=${WEB}&env=prod&by=browser&open=Chrome`);
    await expect(page.locator('[data-rum-attribution-drawer]')).toBeVisible();
    await expect(page.locator('[data-rum-attribution-alert]')).toHaveCount(0);

    await page.goto(`/rum/performance?app=${WEB}&env=prod&by=version&open=2.4.1`);
    await expect(page.locator('[data-rum-attribution-alert]')).toHaveAttribute('href', /version=2\.4\.1/);
  });

  test('open the rule editor on the figure from there, with the application, the environment and the page filled in', async ({ page, mockServer }) => {
    await boot(page, mockServer.port);
    await page.goto(`/rum/performance?app=${WEB}&env=prod&open=%2Fcheckout`);
    // The page writes its time range into the address a moment after it opens: a click made before
    // that would be written over by it.
    await expect(page).toHaveURL(/[?&]time=/);
    await page.locator('[data-rum-attribution-alert]').click();

    await expect(page).toHaveURL(/\/alerts\/rules\/new\?source=rum/);
    await expect(page.locator('[data-alert-source="rum"]')).toHaveClass(/bg-indigo-dim/);
    await expect(page.locator('[data-alert-rum-source]').getByRole('combobox', { name: 'Page', exact: true })).toContainText('/checkout');
    await expect(page.locator('[data-alert-rum-source]').getByRole('combobox', { name: 'Figure', exact: true })).toContainText('LCP (p75)');
  });
});
