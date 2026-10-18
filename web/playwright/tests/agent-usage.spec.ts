import type { Page } from '@playwright/test';

import { MOCK_USAGE_TODAY_TOKENS } from '../fixtures/mockAgentUsage';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

/** Sets the organization's own daily limit and what a chat does at it, on the Usage page. */
async function setBudget(page: Page, limit: number, mode: 'Stop calling the model' | 'Answer and only warn') {
  await page.goto('/agent/settings/usage');
  const form = page.locator('[data-agent-usage-budget]');
  await form.getByRole('combobox', { name: 'Daily token limit' }).click();
  await page.getByRole('option', { name: 'Custom' }).click();
  await form.getByLabel('Tokens').fill(String(limit));
  await form
    .getByRole('combobox', { name: 'When the budget is used up, chats and on-demand analyses' })
    .click();
  await page.getByRole('option', { name: mode }).click();
  await form.getByRole('button', { name: 'Save budget' }).click();
  await expect(page.getByText('Budget saved')).toBeVisible();
}

async function ask(page: Page, question: string) {
  await page.goto('/agent/chat');
  const composer = page.getByTestId('composer-input');
  await composer.fill(question);
  await page.getByRole('button', { name: 'Send' }).click();
}

test.describe('Mole Agent usage and budget', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('the usage page says what today used against the limit, by source and by day', async ({
    page,
  }) => {
    await page.goto('/agent/settings/usage');

    const today = page.locator('[data-agent-usage-today]');
    await expect(today).toHaveAttribute('data-agent-usage-today', 'ok');
    await expect(today).toContainText(MOCK_USAGE_TODAY_TOKENS.toLocaleString('en-US'));
    // The instance default applies while the organization has no limit of its own.
    await expect(today).toContainText('200,000');
    await expect(today.getByRole('meter')).toHaveAttribute('aria-valuenow', '21');
    for (const source of ['Chat', 'Root cause analysis', 'Log inspections', 'LLM functions']) {
      await expect(today).toContainText(source);
    }

    // Days with calls, newest first; the day is the UTC day the count belongs to.
    const rows = page.locator('[data-agent-usage-history] tbody tr');
    await expect(rows).toHaveCount(3);
    await expect(rows.first()).toContainText('2026-05-23');
    await expect(page.locator('[data-agent-usage-history] thead')).toContainText('Cost');

    // Some calls went to a model provider without prices: those costs are a floor, said once.
    await expect(today).toContainText('≥ $0.15');
    await expect(rows.first()).toContainText('≥ $0.15');
    await expect(rows.nth(1)).toContainText('$0.08');
    await expect(rows.nth(1)).not.toContainText('≥');
    await expect(page.locator('[data-agent-usage-unpriced]')).toHaveText(
      'Costs marked ≥ leave out calls to model providers without prices (43 in the last 30 days).',
    );

    const limit = page
      .locator('[data-agent-usage-budget]')
      .getByRole('combobox', { name: 'Daily token limit' });
    await expect(limit).toContainText('Instance default (200,000 tokens)');
  });

  test('a chat past a blocking budget is not sent and the question stays in the composer', async ({
    page,
  }) => {
    await setBudget(page, 40_000, 'Stop calling the model');
    await expect(page.locator('[data-agent-usage-today]')).toHaveAttribute(
      'data-agent-usage-today',
      'exhausted',
    );

    await ask(page, 'Why is checkout slow?');

    const notice = page.locator('[data-agent-budget-notice]');
    await expect(notice).toHaveAttribute('data-agent-budget-notice', 'blocked');
    await expect(notice).toContainText('this question was not sent');
    await expect(notice.getByRole('link', { name: 'View usage' })).toHaveAttribute(
      'href',
      '/agent/settings/usage',
    );
    await expect(page.getByTestId('composer-input')).toHaveText('Why is checkout slow?');
    await expect(page.locator('[data-message-role="assistant"]')).toHaveCount(0);
  });

  test('a chat past a warning-only budget is answered and says it is past the budget', async ({
    page,
  }) => {
    await setBudget(page, 40_000, 'Answer and only warn');
    await ask(page, 'Why is checkout slow?');

    await expect(
      page.getByText('The selected data scope is available.').first(),
    ).toBeVisible();
    await expect(page.locator('[data-agent-budget-notice]')).toHaveAttribute(
      'data-agent-budget-notice',
      'exhausted',
    );
  });

  test('a chat close to the limit is answered with a warning', async ({ page }) => {
    await setBudget(page, 50_000, 'Stop calling the model');
    await ask(page, 'Why is checkout slow?');

    await expect(
      page.getByText('The selected data scope is available.').first(),
    ).toBeVisible();
    const notice = page.locator('[data-agent-budget-notice]');
    await expect(notice).toHaveAttribute('data-agent-budget-notice', 'warning');
    await expect(notice).toContainText('82%');
  });

  test('an answer can be rated helpful, then not helpful with a reason', async ({ page }) => {
    await ask(page, 'Why is checkout slow?');
    await expect(
      page.getByText('The selected data scope is available.').first(),
    ).toBeVisible();

    const feedback = page.locator('[data-agent-feedback="chat_message"]');
    await feedback.getByRole('button', { name: 'Helpful', exact: true }).click();
    await expect(feedback).toHaveAttribute('data-agent-feedback-rating', 'up');

    await feedback.getByRole('button', { name: 'Not helpful' }).click();
    const form = page.locator('[data-agent-feedback-form]');
    await form.getByRole('button', { name: 'Did not find the problem' }).click();
    await form.getByLabel('Comment').fill('It was the database pool.');
    await form.getByRole('button', { name: 'Submit' }).click();

    await expect(feedback).toHaveAttribute('data-agent-feedback-rating', 'down');
    await expect(page.getByText('Thanks for the feedback')).toBeVisible();
  });

  test('a root cause analysis asked for now can be rated not helpful with a reason', async ({
    page,
  }) => {
    await page.goto('/alerts/incidents/inc-2291');
    await page.getByTestId('incident-rca-generate').click();
    await expect(page.getByText('The edge proxy rollout at 09:04', { exact: false })).toBeVisible();

    const feedback = page.locator('[data-agent-feedback="incident_rca"]');
    await feedback.getByRole('button', { name: 'Not helpful' }).click();
    const form = page.locator('[data-agent-feedback-form]');
    await form.getByRole('button', { name: 'Wrong conclusion or cause' }).click();
    await form.getByRole('button', { name: 'Submit' }).click();

    await expect(feedback).toHaveAttribute('data-agent-feedback-rating', 'down');
  });

  test('a root cause analysis is not generated past a blocking budget', async ({ page }) => {
    await setBudget(page, 40_000, 'Stop calling the model');
    await page.goto('/alerts/incidents/inc-2291');
    await page.getByTestId('incident-rca-generate').click();

    await expect(
      page.getByText("Today's Mole Agent token budget for the organization is used up", {
        exact: false,
      }),
    ).toBeVisible();
    await expect(page.locator('[data-agent-feedback="incident_rca"]')).toHaveCount(0);
  });

  test('a model provider carries its no-response timeout and optional prices', async ({ page }) => {
    await page.goto('/agent/settings/models');
    await page.getByRole('button', { name: 'Add model provider' }).click();

    const drawer = page.getByRole('dialog');
    await expect(drawer.getByLabel('No-response timeout (ms)')).toHaveValue('120000');
    await expect(drawer.getByText('Failed connections, rate limits and temporary outages are retried twice.', { exact: false })).toBeVisible();
    // Prices are in the currency chosen for the provider, input cached and not apart.
    await expect(drawer.getByLabel('Input price, not cached ($ per million tokens)')).toHaveValue('');
    await expect(drawer.getByLabel('Input price, cached ($ per million tokens)')).toHaveValue('');
    await expect(drawer.getByLabel('Output price ($ per million tokens)')).toHaveValue('');
  });
});
