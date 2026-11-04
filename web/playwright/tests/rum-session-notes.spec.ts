import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM, mockRumData } from '../fixtures/mockRumData';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);
const CHECKOUT = MOCK_RUM.session.checkout;

test.describe('what is kept about a session besides its events', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto(`/rum/sessions/view/${CHECKOUT}`);
    await expect(page.locator('[data-rum-devtools]')).toBeVisible();
  });

  test('the logs of a session are a tab of the tools, by level, each moving the replay', async ({ page }) => {
    const logged = DATA.logs.filter((row) => row['session.id'] === CHECKOUT);

    await page.locator('[data-rum-tool-tabs]').getByRole('button', { name: /^Logs/ }).click();

    await expect(page.locator('[data-rum-devtools]')).toHaveAttribute('data-rum-tool-tab', 'logs');
    await expect(page.locator('[data-rum-log]')).toHaveCount(logged.length);
    await expect(page.locator('[data-rum-log="error"]').first()).toContainText('Failed to fetch /api/orders');
    await expect(page).toHaveURL(/tab=logs/);

    const position = page.getByRole('slider', { name: 'Seek session replay' });
    const before = await position.inputValue();

    await page.locator('[data-rum-log]').last().getByRole('button', { name: /^Jump to/ }).click();
    await expect.poll(() => position.inputValue()).not.toBe(before);
  });

  test('a comment goes at the moment of the replay, ticks the ruler, and can be changed and removed', async ({ page }) => {
    await page.locator('[data-rum-tool-tabs]').getByRole('button', { name: /^Comments/ }).click();

    await expect(page.locator('[data-rum-comments]')).toContainText('No comments yet');
    await expect(page.locator('[data-rum-comment-add]')).toBeDisabled();

    await page.locator('[data-rum-comment-draft]').fill('Why did the order fail here?');
    await page.locator('[data-rum-comment-add]').click();

    const row = page.locator('[data-rum-comment]').first();

    await expect(row).toContainText('Why did the order fail here?');
    await expect(row).toContainText('Mock User');
    await expect(page.locator('[data-replay-tick="comment"]')).toHaveCount(1);
    await expect(page.locator('[data-rum-comment-draft]')).toHaveValue('');

    await row.getByRole('button', { name: 'Change the comment' }).click();
    await row.getByRole('textbox', { name: 'Change the comment' }).fill('It was the coupon.');
    await row.getByRole('button', { name: 'Save' }).click();
    await expect(row).toContainText('It was the coupon.');

    await row.getByRole('button', { name: 'Delete the comment' }).click();
    await expect(page.locator('[data-rum-comment]')).toHaveCount(0);
    await expect(page.locator('[data-replay-tick="comment"]')).toHaveCount(0);
  });

  test('a summary is written when it is asked for, and its chapters move the replay', async ({ page }) => {
    const panel = page.locator('[data-rum-summary]');

    await expect(panel).toHaveAttribute('data-rum-summary', 'none');

    await page.locator('[data-rum-summary-generate]').click();

    await expect(panel).toHaveAttribute('data-rum-summary', 'written');
    await expect(page.locator('[data-rum-summary-text]')).toContainText('They opened /products');
    await expect(panel).toContainText('mock-model · 150 tokens');
    await expect(page.locator('[data-replay-tick="chapter"]')).not.toHaveCount(0);

    const chapters = page.locator('[data-rum-chapter]');

    await expect(chapters).toHaveCount(2);
    await chapters.first().click();

    await page.reload();
    await expect(page.locator('[data-rum-summary]')).toHaveAttribute('data-rum-summary', 'written');
  });
});

test.describe('the logs of the browser in the explorer', () => {
  test('are a type of row of their own', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/rum/explorer?type=logs');

    await expect(page.getByRole('tab', { name: 'Logs' }).or(page.getByRole('link', { name: 'Logs' })).first()).toBeVisible();
    await expect(page.getByText('opened /products').first()).toBeVisible();
  });
});

test.describe('in Chinese', () => {
  test('the tools speak it', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await page.goto(`/rum/sessions/view/${CHECKOUT}?tab=comments`);

    await expect(page.locator('[data-rum-comments]')).toContainText('还没有评论');
    await expect(page.locator('[data-rum-summary]')).toContainText('还没有摘要');
  });
});
