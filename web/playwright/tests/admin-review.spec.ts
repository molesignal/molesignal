/** Who did what, and which tokens deserve a second look. */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('the audit trail', () => {
  test('reads an action in words and filters by picking', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/settings/audit');

    await expect(page.locator('table tbody tr').first()).toContainText('Agent chat archived');
    await expect(page.locator('table tbody tr').first().locator('[title="agent.chat.archived"]')).toBeVisible();

    const asked = page.waitForRequest(
      (request) => request.url().includes('/api/v1/audit') && request.url().includes('action=agent.chat.archived'),
    );
    await page.getByRole('combobox', { name: 'Action' }).click();
    await page.getByRole('option', { name: 'Agent chat archived' }).click();
    await asked;
  });

  test('colours a status by what it says, and leaves the rest plain', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    const event = (id: string, status: string) => ({
      id,
      orgId: 'default',
      actorKind: 'user',
      actorId: 'dev',
      action: 'agent.execution.completed',
      targetKind: 'agent_execution',
      targetId: id,
      ip: '127.0.0.1',
      userAgent: 'mock',
      payload: { status },
      ts: Date.now() * 1000,
    });
    await page.route('**/api/v1/audit**', (route) =>
      route.fulfill({
        json: { items: [event('a', 'succeeded'), event('b', 'rejected'), event('c', 'pending')], nextCursor: null, retentionDays: 90 },
      }),
    );

    await page.goto('/settings/audit');

    const status = (word: string) => page.locator('table tbody').getByText(word, { exact: true });
    await expect(status('succeeded')).toHaveClass(/green-ink/);
    await expect(status('rejected')).toHaveClass(/red-ink/);
    await expect(status('pending')).toHaveClass(/text-tx-1/);
  });

  test('says a failure is a failure, not an empty trail', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.route('**/api/v1/audit**', (route) =>
      route.fulfill({ status: 500, contentType: 'application/json', body: JSON.stringify({ message: 'store down' }) }),
    );

    await page.goto('/settings/audit');

    await expect(page.getByText('Could not load the audit trail')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible();
  });
});

test.describe('API tokens', () => {
  test('points out what deserves a second look, and narrows to it', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    const now = Date.parse('2026-05-23T10:00:00Z') * 1000;
    const day = 86_400_000_000;
    const token = (id: string, fields: Record<string, unknown>) => ({
      id,
      name: id,
      prefix: id,
      tokenKind: 'service_account',
      userId: 'u-1',
      revoked: false,
      createdAt: now - day,
      expiresAt: now + 365 * day,
      lastUsedAt: now - day,
      policyId: 'read-only',
      ...fields,
    });
    await page.route('**/api/v1/auth/tokens', (route) =>
      route.fulfill({
        json: [
          token('fine', {}),
          token('forever', { expiresAt: null }),
          token('forgotten', { lastUsedAt: now - 120 * day }),
        ],
      }),
    );

    await page.goto('/iam/api-tokens');

    await expect(page.locator('table tbody tr')).toHaveCount(3);
    await page.getByRole('button', { name: /Never expires/ }).click();
    await expect(page.locator('table tbody tr')).toHaveCount(1);
    await expect(page.locator('table tbody tr')).toContainText('forever');
  });
});

test.describe('what cannot be taken back', () => {
  test('deleting an encryption key asks for its name', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.route('**/api/v1/cipher_keys', (route) =>
      route.fulfill({ json: [{ name: 'billing-key', kind: 'aes256', createdAt: 1 }] }),
    );

    await page.goto('/settings/cipher_keys');
    await page.locator('table tbody tr').first().getByRole('button').last().click();

    const dialog = page.getByRole('dialog');
    const action = () => dialog.getByRole('button', { name: 'Delete', exact: true });
    await expect(dialog.getByText('Type billing-key to confirm')).toBeVisible();
    await expect(action()).toBeDisabled();

    await dialog.getByTestId('confirm-type-name').fill('billing');
    await expect(action()).toBeDisabled();

    await dialog.getByTestId('confirm-type-name').fill('billing-key');
    await expect(action()).toBeEnabled();
  });
});
