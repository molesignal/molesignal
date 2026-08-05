import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Mole Intelligence chat workspace', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('centers the investigation action and keeps advanced controls out of the default path', async ({
    page,
  }) => {
    await page.goto('/intelligence/chat');

    await expect(
      page.getByRole('heading', {
        name: 'What would you like to investigate today?',
      }),
    ).toBeVisible();
    await expect(page.getByText('Current context')).toHaveCount(0);
    await expect(page.getByRole('combobox', { name: 'Time' })).toContainText(
      'Last 1h',
    );
    await expect(
      page.getByRole('combobox', { name: 'Mode', exact: true }),
    ).toContainText('Auto mode');
    await expect(
      page.getByRole('combobox', { name: 'Execution' }),
    ).toContainText('Advice only');
    await expect(page.getByRole('combobox', { name: 'Model' })).toHaveCount(0);
    await expect(page.getByRole('combobox', { name: 'Prompt' })).toHaveCount(0);

    const composer = page.getByLabel(
      'Ask about an anomaly, service status, or enter an operations task…',
    );
    const collapsedHeight = await composer.evaluate(
      (element) => element.getBoundingClientRect().height,
    );
    await composer.fill(['one', 'two', 'three', 'four', 'five'].join('\n'));
    const expandedHeight = await composer.evaluate(
      (element) => element.getBoundingClientRect().height,
    );
    expect(expandedHeight).toBeGreaterThan(collapsedHeight);
    await composer.fill('');
    const resetHeight = await composer.evaluate(
      (element) => element.getBoundingClientRect().height,
    );
    expect(resetHeight).toBeLessThanOrEqual(collapsedHeight + 1);
    await expect(page.getByTestId('composer-controls')).not.toHaveClass(
      /border-t/,
    );

    await page.getByRole('button', { name: 'Add context' }).click();
    await page.getByLabel('Service', { exact: true }).fill('checkout-api');
    await expect(
      page.getByTestId('composer-context').getByText('Service: checkout-api'),
    ).toBeVisible();
    await page.keyboard.press('Escape');

    await page.getByRole('button', { name: 'More' }).click();
    await expect(page.getByRole('combobox', { name: 'Agent Profile' })).toBeVisible();
    await expect(page.getByRole('combobox', { name: 'Model' })).toBeVisible();
    await expect(page.getByRole('combobox', { name: 'Prompt' })).toBeVisible();
    await expect(page.getByText('Investigation limit')).toBeVisible();
    await page.keyboard.press('Escape');

    await page
      .getByRole('button', {
        name: /Why is web's error rate increasing\?/,
      })
      .click();
    await expect(
      page.getByLabel(
        'Ask about an anomaly, service status, or enter an operations task…',
      ),
    ).toHaveValue("Why is web's error rate increasing?");
    await expect(
      page.getByRole('button', {
        name: /Analyze anomalous traces from the last 30 minutes/,
      }),
    ).toBeVisible();
    await expect(
      page.getByRole('combobox', { name: 'Mode', exact: true }),
    ).toContainText('Deep investigation');
  });

  test('renders product evidence and regenerates an answer without duplicating the question', async ({
    page,
  }) => {
    await page.goto('/intelligence/chat');

    const initialComposerBox = await page
      .getByTestId('composer-shell')
      .boundingBox();
    await page
      .getByRole('button', {
        name: /Who is on call for production\?/,
      })
      .click();
    const sendButton = page.getByRole('button', { name: 'Send' });
    await expect(sendButton).toHaveClass(/rounded-full/);
    expect((await sendButton.textContent())?.trim()).toBe('');
    await sendButton.click();

    const stopButton = page.getByRole('button', { name: 'Stop' });
    await expect(stopButton).toHaveClass(/rounded-full/);
    expect((await stopButton.textContent())?.trim()).toBe('');

    await expect(page.locator('[data-message-role="user"]')).toHaveCount(1);
    await expect(
      page.getByText('The selected data scope is available for investigation.'),
    ).toBeVisible();
    await expect(page.getByText('Investigation chat')).toBeVisible();
    await expect(page.getByText('Checked 1 data sources')).toHaveCount(0);
    const investigationProcess = page.getByTestId('investigation-process');
    await expect(investigationProcess).not.toHaveClass(/border|bg-bg-1|rounded/);
    const investigationSummary = investigationProcess.locator('summary').first();
    await expect(investigationSummary).toContainText('Processed');
    await expect(page.getByText('list_streams')).not.toBeVisible();
    const historyComposerBox = await page
      .getByTestId('composer-shell')
      .boundingBox();
    expect(initialComposerBox).not.toBeNull();
    expect(historyComposerBox).not.toBeNull();
    expect(
      Math.abs(
        (historyComposerBox?.y ?? 0) +
          (historyComposerBox?.height ?? 0) -
          ((initialComposerBox?.y ?? 0) + (initialComposerBox?.height ?? 0)),
      ),
    ).toBeLessThan(2);

    await investigationSummary.click();
    await expect(page.getByText('Check available data')).toBeVisible();
    await expect(page.getByText('list_streams')).not.toBeVisible();
    await page.getByText('Technical detail').click();
    await expect(page.getByText('list_streams')).toBeVisible();

    await page.getByRole('button', { name: 'Regenerate' }).click();
    await expect(page.getByText('Answer 2 / 2')).toBeVisible();
    await expect(page.locator('[data-message-role="user"]')).toHaveCount(1);
  });

  test('uses an in-app confirmation dialog when deleting chat history', async ({
    page,
  }) => {
    let nativeDialogCount = 0;
    page.on('dialog', async (dialog) => {
      nativeDialogCount += 1;
      await dialog.dismiss();
    });

    await page.goto('/intelligence/chat');
    await page
      .getByRole('button', {
        name: /Who is on call for production\?/,
      })
      .click();
    await page.getByRole('button', { name: 'Send' }).click();
    await expect(
      page.getByText('The selected data scope is available for investigation.'),
    ).toBeVisible();

    await page.getByRole('button', { name: 'Delete chat' }).click();
    const confirmDialog = page.getByRole('dialog', { name: 'Delete chat?' });
    await expect(confirmDialog).toBeVisible();
    await expect(
      confirmDialog.getByText(
        '“Who is on call for production?” will be archived before it is removed from chat history.',
      ),
    ).toBeVisible();
    expect(nativeDialogCount).toBe(0);

    await confirmDialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(confirmDialog).not.toBeVisible();
    await expect(page.getByRole('button', { name: 'Delete chat' })).toBeVisible();

    await page.getByRole('button', { name: 'Delete chat' }).click();
    await page
      .getByRole('dialog', { name: 'Delete chat?' })
      .getByRole('button', { name: 'Delete', exact: true })
      .click();
    await expect(page.getByRole('dialog', { name: 'Delete chat?' })).not.toBeVisible();
    await expect(page.getByRole('button', { name: 'Delete chat' })).toHaveCount(0);
    expect(nativeDialogCount).toBe(0);
  });

  test('matches the standard alerts header height and uses rounded active tabs', async ({
    page,
  }) => {
    await page.goto('/intelligence/chat');
    const intelligenceHeader = await page
      .getByTestId('intelligence-module-header')
      .boundingBox();

    await page.route('**/api/v1/alerts/incidents**', (route) =>
      route.fulfill({ json: [] }),
    );
    await page.route('**/api/v1/alerts/rules**', (route) =>
      route.fulfill({ json: [] }),
    );
    await page.goto('/alerts/incidents');
    const alertTitle = page
      .locator('.type-page-title')
      .filter({ hasText: 'Alert incidents' });
    await expect(alertTitle).toBeVisible();
    const pageHeader = await alertTitle
      .locator('..')
      .locator('..')
      .locator('..')
      .boundingBox();
    const alertsSubnav = await page.getByTestId('alerts-subnav').boundingBox();
    const activeTab = page
      .getByTestId('alerts-subnav')
      .locator('a[href="/alerts/incidents"]');

    expect(intelligenceHeader).not.toBeNull();
    expect(pageHeader).not.toBeNull();
    expect(alertsSubnav).not.toBeNull();
    expect(
      Math.abs(
        (intelligenceHeader?.height ?? 0) -
          ((pageHeader?.height ?? 0) + (alertsSubnav?.height ?? 0) - 1),
      ),
    ).toBeLessThan(2);
    await expect(activeTab).toHaveClass(/rounded-md/);
    await expect(activeTab).toHaveClass(/bg-bg-2/);
    await expect(activeTab).not.toHaveClass(/border-b-2/);
  });
});
