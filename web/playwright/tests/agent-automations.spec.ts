import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Mole Agent automations', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('lists what starts each automation and how its last run went, with the runs behind it', async ({
    page,
  }) => {
    await page.goto('/agent/automations');

    const health = page.locator('[data-agent-automation="automation-daily-health"]').locator('xpath=ancestor::tr');
    await expect(health).toContainText('Daily health check');
    await expect(health).toContainText('Weekdays 09:00');
    await expect(health.locator('[data-automation-run-status="succeeded"]')).toHaveText('Succeeded');

    const incident = page
      .locator('[data-agent-automation="automation-incident-analysis"]')
      .locator('xpath=ancestor::tr');
    await expect(incident).toContainText('New critical, error incidents');
    await expect(incident.locator('[data-automation-run-status="blocked"]')).toHaveText('Needs someone');

    await page.getByRole('button', { name: 'Runs of Incident analysis' }).click();
    const runs = page.getByRole('dialog');
    await expect(runs.locator('[data-automation-run="run-incident-1"]')).toContainText(
      'An MCP server needs the credential of the person it runs as',
    );
    await expect(runs.getByRole('link', { name: 'Open the run' })).toHaveAttribute(
      'href',
      '/agent/chat?chat=chat-automation-incident-1',
    );
  });

  test('starts a new automation from a template and saves it', async ({ page }) => {
    await page.goto('/agent/automations');
    await page.getByRole('button', { name: 'New automation' }).click();

    await page.locator('[data-automation-template="daily_health"]').click();
    const editor = page.locator('[data-automation-editor="new"]');
    await expect(editor.getByLabel('Name')).toHaveValue('Daily health check');
    await expect(editor.getByRole('button', { name: 'Weekdays 09:00' })).toHaveAttribute('aria-pressed', 'true');

    await editor.getByLabel('Name').fill('Payments health check');
    const created = page.waitForRequest(
      (request) => request.method() === 'POST' && request.url().endsWith('/api/v1/agent/automations'),
    );
    await page.getByRole('dialog').getByRole('button', { name: 'Create' }).click();

    const body = (await created).postDataJSON();
    expect(body).toMatchObject({
      name: 'Payments health check',
      scope: 'personal',
      template: 'daily_health',
      trigger: { kind: 'schedule', cron: '0 9 * * 1-5' },
    });
    await expect(page.getByText('Payments health check')).toBeVisible();
  });

  test('runs an automation by hand and shows the run as it goes', async ({ page }) => {
    await page.goto('/agent/automations');
    await page.getByRole('button', { name: 'Run Daily health check now' }).click();

    await expect(page.getByText('Daily health check is running')).toBeVisible();
    const runs = page.getByRole('dialog');
    await expect(runs.locator('[data-automation-run-status="running"]').first()).toBeVisible();
    await expect(runs.getByText('All checkout services are healthy.')).toBeVisible({ timeout: 15_000 });
  });

  test('makes an API token once and shows the call a CI/CD job sends', async ({ page }) => {
    await page.goto('/agent/automations');
    await page.getByRole('button', { name: 'More for Change impact check' }).click();
    await page.getByRole('menuitem', { name: 'API token' }).click();

    const dialog = page.getByRole('dialog');
    await expect(dialog).toContainText('A token is set (mspat-x9qe2k7m2q…)');
    await dialog.getByRole('button', { name: 'Make a new token' }).click();

    const token = page.locator('[data-automation-token]');
    await expect(token).toContainText('mspat-mocktoken');
    await expect(token).toContainText(
      '/api/v1/public/automations/automation-change-impact/trigger',
    );
    await expect(token).toContainText('Authorization: Bearer mspat-mocktoken');
  });
});
