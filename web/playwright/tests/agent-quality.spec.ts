import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Mole Agent quality', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('sums up the period, each rule and each source', async ({ page }) => {
    await page.goto('/agent/quality');

    const totals = page.locator('[data-quality-totals]');
    await expect(totals.locator('[data-fact="answers"]')).toHaveText(/Answers scored\s*42/);
    await expect(totals.locator('[data-fact="evaluators"]')).toHaveText(/Evaluators\s*71/);
    await expect(totals.locator('[data-fact="reviews"]')).toHaveText(/Waiting for review\s*2/);
    await expect(page.locator('[data-quality-dimension="evidence"]')).toContainText('74');
    await expect(page.getByRole('region', { name: 'By source' })).toContainText('Team chats');
    await expect(page.getByRole('region', { name: 'By prompt version' })).toContainText('System default v1');

    await totals.getByRole('button', { name: /Waiting for review/ }).click();
    await expect(page).toHaveURL(/\/agent\/quality\/reviews$/);
  });

  test('lists scored answers and opens one with its rules, evaluators and review', async ({ page }) => {
    await page.goto('/agent/quality/answers');

    const row = page
      .locator('[data-quality-answer="message-disk-guess"]')
      .locator('xpath=ancestor::tr');
    await expect(row).toContainText('Is the disk of db-2 filling up?');
    await expect(row.locator('td').nth(2)).toHaveText('25');

    await row.click();
    const drawer = page.getByRole('dialog', { name: 'Scored answer' });
    await expect(drawer.locator('[data-quality-rule="query_validity"]')).toContainText(
      '1 of 2 queries do not parse',
    );
    await expect(drawer.locator('[data-quality-rule="feedback"]')).toContainText(
      'Rated not helpful: Wrong conclusion or cause',
    );
    await expect(drawer.locator('[data-quality-evaluator-score="evaluator-accuracy"]')).toContainText(
      'Guesses the WAL without reading any disk metric.',
    );
    await expect(drawer.locator('[data-review="review-disk"]')).toContainText('Rated not helpful');
    await expect(drawer.getByRole('link', { name: 'Open the chat' })).toHaveAttribute(
      'href',
      '/agent/chat?chat=chat-disk',
    );
  });

  test('accepts an answer for the regression set with a note', async ({ page }) => {
    await page.goto('/agent/quality/reviews');
    await page.locator('[data-quality-review="review-disk"]').click();

    const drawer = page.getByRole('dialog', { name: 'Scored answer' });
    await drawer.getByLabel('Note').fill('The disk metric shows the logs volume filling up.');

    const decided = page.waitForRequest((request) =>
      request.url().endsWith('/api/v1/agent/quality/reviews/review-disk/decision'),
    );
    await drawer.getByRole('button', { name: 'Accept' }).click();

    expect((await decided).postDataJSON()).toEqual({
      status: 'accepted',
      note: 'The disk metric shows the logs volume filling up.',
    });
    await expect(page.getByText('Accepted: it joins the regression set')).toBeVisible();
  });

  test('adds an evaluator that scores a sample of team chats and automations', async ({ page }) => {
    await page.goto('/agent/quality/evaluators');
    await expect(page.locator('[data-quality-evaluator="evaluator-accuracy"]')).toContainText('Answer accuracy');

    await page.getByRole('button', { name: 'New evaluator' }).click();
    const editor = page.locator('[data-evaluator-editor="new"]');
    await editor.getByLabel('Name').fill('Evidence check');

    const created = page.waitForRequest(
      (request) => request.method() === 'POST' && request.url().endsWith('/api/v1/agent/quality/evaluators'),
    );
    await page.getByRole('dialog').getByRole('button', { name: 'Add' }).click();

    expect((await created).postDataJSON()).toMatchObject({
      name: 'Evidence check',
      targets: ['team_chat', 'automation'],
      samplePercent: 20,
      temperature: 0,
      enabled: true,
    });
    await expect(page.locator('[data-quality-evaluator="evaluator-2"]')).toContainText('Evidence check');
  });

  test('checks an organization prompt before it becomes the default', async ({ page }) => {
    await page.goto('/agent/settings/prompts');

    const builtin = page.locator('article').filter({ hasText: 'Mole Agent system instruction' });
    await builtin.getByRole('button', { name: 'Create override' }).click();
    const drawer = page.getByRole('dialog', { name: 'Create prompt override' });
    await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('Cite every claim');
    await drawer.getByRole('button', { name: 'Save' }).click();

    const promotion = page.getByRole('dialog', { name: 'Set Cite every claim as the default' });
    await expect(promotion.locator('[data-prompt-gate-reason="prompt_check_required"]')).toBeVisible();
    await promotion.getByRole('button', { name: 'Run the check' }).click();

    await expect(promotion.locator('[data-prompt-check-progress]')).toContainText('of 3');
    await expect(promotion.locator('[data-prompt-check-verdict="better"]')).toContainText(
      'this version scored 83 and the present default 75',
      { timeout: 15_000 },
    );
    await expect(promotion).toContainText('From the regression set');

    const set = page.waitForRequest((request) => request.url().endsWith('/set-default'));
    await promotion.getByRole('button', { name: 'Set as default' }).click();
    expect((await set).postDataJSON()).toEqual({ confirm: false });
    await expect(page.locator('article').filter({ hasText: 'Cite every claim' }).getByText('Effective')).toBeVisible();
  });
});
