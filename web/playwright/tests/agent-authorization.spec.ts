import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

/** Ask about Linear: its tool waits for the member's own credential. */
async function askLinear(page: Page) {
  await page.goto('/agent/chat');
  const box = page.getByTestId('composer-input');
  await box.fill('Find the Linear issues about payments');
  await box.press('Enter');

  const card = page.locator('[data-agent-authorization]');
  await expect(card).toBeVisible();
  return card;
}

test.describe('Mole Agent authorize to continue', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('asks for the member credential in the chat and goes on once it is saved', async ({
    page,
  }) => {
    const card = await askLinear(page);
    await expect(card).toContainText('Authorize to continue');
    await expect(card).toContainText('needs your own credential for Linear');
    await expect(card.locator('[data-agent-authorization-instructions]')).toHaveText(
      'Create a personal API key under Linear → Settings → API.',
    );
    await expect(card).toContainText('Sent as Bearer <credential> in the authorization header.');
    await expect(page.locator('[data-run-status="waiting_input"]')).toBeVisible();

    const saved = page.waitForRequest(
      (request) =>
        request.method() === 'PUT' &&
        request.url().endsWith('/api/v1/agent/mcp-servers/mcp-linear/my-credential'),
    );
    const responded = page.waitForRequest(
      (request) => request.method() === 'POST' && /\/agent\/runs\/[^/]+\/respond$/.test(request.url()),
    );
    await card.getByLabel('Your credential for Linear').fill('lin_api_f00d');
    await card.getByRole('button', { name: 'Save and continue' }).click();

    expect((await saved).postDataJSON()).toEqual({ value: 'lin_api_f00d' });
    expect((await responded).postDataJSON()).toMatchObject({ authorized: true });
    await expect(card).toHaveCount(0);
    await expect(page.getByText('Linear has two open issues about payments.')).toBeVisible();
  });

  test('a skip leaves the server out of the answer and saves nothing', async ({ page }) => {
    let saved = false;
    page.on('request', (request) => {
      if (request.method() === 'PUT' && request.url().includes('/my-credential')) saved = true;
    });

    const card = await askLinear(page);
    await card.getByRole('button', { name: 'Skip' }).click();

    await expect(card).toHaveCount(0);
    await expect(page.getByText('Linear was left out of this answer.')).toBeVisible();
    expect(saved).toBe(false);
  });
});
