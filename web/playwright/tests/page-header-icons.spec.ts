import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Analysis module title bars', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('uses compact module icons on shared and Mole Agent headers', async ({
    page,
  }) => {
    await page.goto('/metrics');

    const pageHeader = page.getByTestId('page-header');
    const pageIcon = page.getByTestId('page-header-module-icon');
    const pageHeaderBox = await pageHeader.boundingBox();
    const pageIconBox = await pageIcon.boundingBox();

    expect(pageHeaderBox).not.toBeNull();
    expect(pageHeaderBox!.height).toBeLessThanOrEqual(52);
    expect(pageIconBox).not.toBeNull();
    expect(pageIconBox!.width).toBe(pageIconBox!.height);

    await page.goto('/agent/chat');

    const agentTitleRowBox = await page
      .getByTestId('agent-title-row')
      .boundingBox();
    const agentIconBox = await page
      .getByTestId('agent-module-icon')
      .boundingBox();

    expect(agentTitleRowBox).not.toBeNull();
    expect(agentTitleRowBox!.height).toBeLessThanOrEqual(52);
    expect(agentIconBox).not.toBeNull();
    expect(agentIconBox!.width).toBe(agentIconBox!.height);
    expect(agentIconBox!.width).toBe(pageIconBox!.width);
  });
});
