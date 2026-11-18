import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Mole Agent quality: data gaps and drills', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('lists the data gaps by kind with the answers that found them', async ({ page }) => {
    await page.goto('/agent/quality/gaps');

    const totals = page.locator('[data-quality-gap-totals]');
    await expect(totals.locator('[data-fact="answers"]')).toHaveText(/Answers with tool calls\s*24/);
    await expect(totals.locator('[data-fact="with-gaps"]')).toHaveText(/With a gap\s*6 · 25%/);

    const dataMap = page.locator('[data-quality-gap-kind="data_map"]');
    await expect(dataMap).toContainText('Where the data is');
    await expect(dataMap).toContainText('nginx queried 11 times, 8 empty or failed');
    await expect(dataMap).toContainText('3 answers');

    await expect(page.locator('[data-quality-gap-kind="field_alignment"]')).toContainText(
      'order-api has data in metrics or APM, but a query of app_logs by that name found nothing',
    );
    await expect(page.locator('[data-quality-gap-kind="feature_tools"]')).toContainText(
      'Raw logs of nginx pulled 3 times without their patterns',
    );

    await page.locator('[data-quality-gap-example="message-disk-guess"]').click();
    const drawer = page.getByRole('dialog', { name: 'Scored answer' });
    await expect(drawer.getByRole('link', { name: 'Open the chat' })).toHaveAttribute(
      'href',
      '/agent/chat?chat=chat-disk',
    );
  });

  test('makes a drill from a postmortem’s root cause', async ({ page }) => {
    await page.goto('/agent/quality/drills');

    await expect(page.locator('[data-quality-drill="drill-pool"]')).toContainText('Checkout 5xx above 5%');
    const pool = page.locator('[data-quality-drill="drill-pool"]').locator('xpath=ancestor::tr');
    await expect(pool.locator('[data-quality-drill-last="done"]')).toContainText('75');
    const dns = page.locator('[data-quality-drill="drill-dns"]').locator('xpath=ancestor::tr');
    await expect(dns).toContainText('Not run yet');

    await page.getByRole('button', { name: 'New drill' }).click();
    const drawer = page.getByRole('dialog', { name: 'New drill' });
    await expect(drawer).not.toContainText('Checkout 5xx above 5%');
    await drawer.getByText('Order queue backlog over 10k').click();
    await expect(drawer.locator('[data-new-drill-reference]')).toContainText(
      'A consumer deploy left the order workers with one thread each.',
    );

    const created = page.waitForRequest(
      (request) => request.method() === 'POST' && request.url().endsWith('/api/v1/agent/quality/drills'),
    );
    await drawer.getByRole('button', { name: 'Create' }).click();

    expect((await created).postDataJSON()).toEqual({ incidentId: 'incident-queue' });
    await expect(page.getByText('Drill “Order queue backlog over 10k” created')).toBeVisible();
    await expect(page.locator('[data-quality-drill="drill-3"]')).toContainText('Order queue backlog over 10k');
  });

  test('writes the reference root cause of another incident by hand', async ({ page }) => {
    await page.goto('/agent/quality/drills');
    await page.getByRole('button', { name: 'New drill' }).click();

    const drawer = page.getByRole('dialog', { name: 'New drill' });
    await drawer.getByText('Another incident').click();
    await drawer.getByLabel('Incident ID').fill('incident-cache');
    await expect(drawer.getByRole('button', { name: 'Create' })).toBeDisabled();
    await drawer.getByRole('textbox', { name: 'Root cause' }).fill('The cache cluster lost a node and every miss went to the database.');

    const created = page.waitForRequest(
      (request) => request.method() === 'POST' && request.url().endsWith('/api/v1/agent/quality/drills'),
    );
    await drawer.getByRole('button', { name: 'Create' }).click();

    expect((await created).postDataJSON()).toEqual({
      incidentId: 'incident-cache',
      reference: 'The cache cluster lost a node and every miss went to the database.',
    });
  });

  test('runs a drill and reads its graded run', async ({ page }) => {
    await page.goto('/agent/quality/drills');

    const started = page.waitForRequest(
      (request) =>
        request.method() === 'POST' && request.url().endsWith('/api/v1/agent/quality/drills/drill-dns/runs'),
    );
    await page.getByRole('button', { name: 'Run the drill “Search timeouts in eu-west”' }).click();
    await started;
    await expect(page.getByText('The drill is running. It is graded once the investigation ends.')).toBeVisible();

    await page.getByRole('button', { name: 'Runs of “Search timeouts in eu-west”' }).click();
    const drawer = page.getByRole('dialog', { name: 'Drill runs' });
    await expect(drawer).toContainText('A DNS resolver of eu-west dropped queries after its config was rolled out.');
    await expect(drawer.locator('[data-quality-drill-reasoning]')).toContainText(
      'Finds the resolver that dropped queries after the rollout.',
      { timeout: 15_000 },
    );
    await expect(drawer.getByRole('link', { name: 'Open the investigation' })).toHaveAttribute(
      'href',
      /\/agent\/chat\?chat=chat-drill-dns-/,
    );
  });

  test('runs every drill again in one batch', async ({ page }) => {
    await page.goto('/agent/quality/drills');

    const batch = page.waitForRequest(
      (request) => request.method() === 'POST' && request.url().endsWith('/api/v1/agent/quality/drills/runs'),
    );
    await page.getByRole('button', { name: 'Run all again' }).click();

    expect((await batch).postDataJSON()).toEqual({});
    await expect(page.getByText('2 drills are running')).toBeVisible();
  });
});
