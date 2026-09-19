import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Product page title bars', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('uses one inline title height with and without module icons', async ({
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

    const metricsTitleRow = pageHeader.locator('[data-page-title-row]');
    await expect(metricsTitleRow).toBeVisible();
    await expect(pageHeader.getByText('·')).toBeVisible();
    const metricsTitleRowBox = await metricsTitleRow.boundingBox();

    await page.goto('/datasource/recommended/kubernetes');

    const datasourceHeader = page.getByTestId('page-header');
    const datasourceTitleRow = datasourceHeader.locator('[data-page-title-row]');
    await expect(datasourceHeader).toHaveAttribute(
      'data-page-header-layout',
      'inline',
    );
    await expect(datasourceHeader.getByText('·')).toBeVisible();
    await expect(page.getByTestId('page-header-module-icon')).toHaveCount(0);
    const datasourceTitleRowBox = await datasourceTitleRow.boundingBox();

    expect(metricsTitleRowBox).not.toBeNull();
    expect(datasourceTitleRowBox).not.toBeNull();
    expect(datasourceTitleRowBox!.height).toBe(metricsTitleRowBox!.height);

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
