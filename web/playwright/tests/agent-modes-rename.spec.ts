import type { Page, Request } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const sentMessage = (request: Request) =>
  request.method() === 'POST' && /\/api\/v1\/agent\/chat\/[^/]+\/messages$/.test(request.url());

/** One default Agent Profile that lets its chats pick Bypass permissions. */
async function allowBypass(page: Page) {
  await page.route('**/api/v1/agent/settings/agent-profiles', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: {
            profiles: [
              {
                id: 'profile-ops',
                name: 'Operations',
                description: '',
                model_provider_id: null,
                model: null,
                allowed_tools: [],
                data_scope: {},
                risk_policy: {},
                network_access: 'blocked',
                max_context_tokens: 32_000,
                max_investigation_secs: 1_800,
                max_tool_calls: 32,
                allow_bypass_permissions: true,
                is_default: true,
                enabled: true,
                created_by: 'dev',
                created_at: 0,
                updated_at: 0,
              },
            ],
          },
        })
      : route.fallback(),
  );
}

test.describe('Mole Agent execution modes', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('Auto is picked at once; Bypass permissions stays locked until the Agent Profile allows it', async ({
    page,
  }) => {
    await page.goto('/agent/chat');
    const trigger = page.getByRole('button', { name: 'Execution' });
    await trigger.click();

    const modes = page.getByRole('menuitemradio');
    await expect(modes).toHaveCount(5);
    const bypass = modes.filter({ hasText: 'Bypass permissions' });
    await expect(bypass).toHaveAttribute('aria-disabled', 'true');
    await expect(bypass).toContainText('An administrator allows it in the Agent Profile first');

    // Its number does nothing while it is locked; Auto's picks it.
    await page.keyboard.press('5');
    await expect(page.getByRole('menu')).toBeVisible();
    await page.keyboard.press('4');
    await expect(trigger).toContainText('Auto mode');
    await expect(trigger).toHaveAttribute('data-agent-execution-mode', 'auto');
  });

  test('Bypass permissions is asked about before it is on, and goes out with the question', async ({
    page,
  }) => {
    await allowBypass(page);
    await page.goto('/agent/chat');
    const trigger = page.getByRole('button', { name: 'Execution' });

    // Given up in the dialog: the mode stays what it was.
    await trigger.click();
    await page.getByRole('menuitemradio').filter({ hasText: 'Bypass permissions' }).click();
    const dialog = page.getByRole('dialog', { name: 'Turn on Bypass permissions?' });
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(trigger).toHaveAttribute('data-agent-execution-mode', 'advice_only');

    await trigger.click();
    await page.getByRole('menuitemradio').filter({ hasText: 'Bypass permissions' }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Turn on' }).click();
    await expect(trigger).toHaveAttribute('data-agent-execution-mode', 'bypass_permissions');

    const sent = page.waitForRequest(sentMessage);
    const box = page.getByTestId('composer-input');
    await box.fill('Restart the checkout pods');
    await box.press('Enter');
    expect((await sent).postDataJSON()).toMatchObject({ execution_policy: 'bypass_permissions' });
  });

  test('/policy bypass says why it cannot be picked where it is locked', async ({ page }) => {
    await page.goto('/agent/chat');
    const box = page.getByTestId('composer-input');
    await box.fill('/policy bypass');
    await box.press('Escape');
    await box.press('Enter');
    await expect(page.locator('[data-agent-command-notice]')).toContainText(
      'An administrator allows it in the Agent Profile first',
    );
    await expect(page.getByRole('button', { name: 'Execution' })).toHaveAttribute(
      'data-agent-execution-mode',
      'advice_only',
    );
  });
});

test.describe('Mole Agent chat names', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('a chat is renamed in the list: Enter keeps the name, Escape leaves it', async ({ page }) => {
    await page.goto('/agent/chat');
    const box = page.getByTestId('composer-input');
    await box.fill('hi');
    await box.press('Enter');
    const title = page.getByTestId('conversation-history-title').first();
    await expect(title).toHaveText('hi');

    const row = page.getByTestId('conversation-history').locator('[data-chat-depth]').first();
    await row.hover();
    await row.getByRole('button', { name: 'Rename' }).click();
    const field = page.getByTestId('conversation-history-rename');
    await expect(field).toBeFocused();
    await expect(field).toHaveValue('hi');

    const renamed = page.waitForRequest(
      (request) => request.method() === 'PATCH' && /\/api\/v1\/agent\/chat\/[^/]+$/.test(request.url()),
    );
    await field.fill('Greetings');
    await field.press('Enter');
    expect((await renamed).postDataJSON()).toEqual({ title: 'Greetings' });
    await expect(title).toHaveText('Greetings');

    await row.hover();
    await row.getByRole('button', { name: 'Rename' }).click();
    await page.getByTestId('conversation-history-rename').fill('Not this');
    await page.getByTestId('conversation-history-rename').press('Escape');
    await expect(title).toHaveText('Greetings');
  });

  test('the way to a new chat sits on the line of the day, not on a row of its own', async ({
    page,
  }) => {
    await page.goto('/agent/chat');
    const box = page.getByTestId('composer-input');
    await box.fill('hi');
    await box.press('Enter');
    await expect(page.getByTestId('conversation-history-title').first()).toHaveText('hi');

    const list = page.getByTestId('conversation-history');
    const [plus, day] = await Promise.all([
      list.getByRole('button', { name: 'New chat' }).boundingBox(),
      list.getByRole('heading', { level: 3 }).first().boundingBox(),
    ]);
    // Both on one line: their middles within a pixel.
    expect(Math.abs(plus!.y + plus!.height / 2 - (day!.y + day!.height / 2))).toBeLessThanOrEqual(1);
  });
});
