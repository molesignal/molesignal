import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Mole Agent operations workspace', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('keeps conversation history, chat workspace, and context visible from the first visit', async ({
    page,
  }) => {
    await page.route('**/api/v1/agent/investigations', (route) =>
      route.fulfill({ json: { investigations: [] } }),
    );
    await page.goto('/agent/chat');

    await expect(
      page.getByRole('heading', {
        name: 'AI-powered assistant',
      }),
    ).toBeVisible();
    await expect(page.getByTestId('conversation-history')).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Chats' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'New chat' })).toBeVisible();
    await expect(page.getByText('No conversations yet')).toBeVisible();
    await expect(page.getByTestId('conversation-context')).toBeVisible();
    await expect(
      page
        .getByTestId('conversation-context')
        .getByRole('heading', { name: 'Context' }),
    ).toBeVisible();
    await expect(page.getByText('Agent status')).toHaveCount(0);
    await expect(page.getByText('System health')).toHaveCount(0);
    await expect(page.getByText('Current context')).toHaveCount(0);

    const workspace = page.locator('[data-agent-chat-workspace]');
    const history = page.getByTestId('conversation-history');
    const conversation = page.getByTestId('conversation-workspace');
    const context = page.getByTestId('conversation-context');
    const workspaceAppearance = await workspace.evaluate((element) => {
      const styles = getComputedStyle(element);
      const colorProbe = document.createElement('span');
      colorProbe.style.color = styles.getPropertyValue('--page-canvas').trim();
      return {
        gap: styles.columnGap,
        background: styles.backgroundColor,
        canvas: colorProbe.style.color,
      };
    });
    expect(workspaceAppearance.gap).toBe('8px');
    expect(workspaceAppearance.background).toBe(workspaceAppearance.canvas);

    for (const pane of [history, conversation, context]) {
      const appearance = await pane.evaluate((element) => {
        const styles = getComputedStyle(element);
        return {
          borderLeft: styles.borderLeftWidth,
          borderRight: styles.borderRightWidth,
          radius: styles.borderRadius,
        };
      });
      expect(appearance.borderLeft).toBe('0px');
      expect(appearance.borderRight).toBe('0px');
      expect(appearance.radius).not.toBe('0px');
    }

    const [historyBackground, conversationBackground, contextBackground] =
      await Promise.all(
        [history, conversation, context].map((pane) =>
          pane.evaluate((element) => getComputedStyle(element).backgroundColor),
        ),
      );
    expect(historyBackground).toBe(contextBackground);
    expect(historyBackground).not.toBe(conversationBackground);

    const contextSections = context.locator('[data-agent-context-section]');
    await expect(contextSections).toHaveCount(2);
    for (const section of await contextSections.all()) {
      await expect(section).not.toHaveClass(/border/);
    }
    for (const link of await context.locator('[data-agent-context-link]').all()) {
      await expect(link).not.toHaveClass(/border/);
    }

    const quickActions = page.getByTestId('agent-quick-actions');
    await expect(quickActions).toHaveClass(/divide-y/);
    const quickActionBorders = await quickActions.evaluate((element) => {
      const styles = getComputedStyle(element);
      return [styles.borderTopWidth, styles.borderBottomWidth];
    });
    expect(quickActionBorders).toEqual(['1px', '1px']);
    await expect(page.getByRole('combobox', { name: 'Time' })).toContainText(
      'Last 1h',
    );
    await expect(
      page.getByRole('combobox', { name: 'Mode', exact: true }),
    ).toHaveCount(0);
    await expect(
      page.getByRole('combobox', { name: 'Execution' }),
    ).toContainText('Advice mode');
    await expect(page.getByRole('combobox', { name: 'Model' })).toHaveCount(0);
    await expect(page.getByRole('combobox', { name: 'Prompt' })).toHaveCount(0);

    const composerShell = page.getByTestId('composer-shell');
    const composerAppearance = await composerShell.evaluate((element) => {
      const styles = getComputedStyle(element);
      const colorProbe = document.createElement('span');
      colorProbe.style.color = styles
        .getPropertyValue('--control-surface')
        .trim();
      return {
        borderTop: styles.borderTopWidth,
        background: styles.backgroundColor,
        controlSurface: colorProbe.style.color,
      };
    });
    expect(composerAppearance.borderTop).toBe('0px');
    expect(composerAppearance.background).toBe(
      composerAppearance.controlSurface,
    );
    const compactComposerBox = await composerShell.boundingBox();
    expect(compactComposerBox?.height).toBeLessThanOrEqual(100);
    const composer = page.getByLabel(
      'Ask about incidents, services, or operational tasks…',
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
    await expect(page.getByText('Agent command bar', { exact: true })).toHaveCount(0);
    await expect(page.getByText(/Enter to send/)).toHaveCount(0);

    await page.getByRole('button', { name: 'Add context' }).click();
    await page.getByLabel('Service', { exact: true }).fill('checkout-api');
    await expect(
      page.getByTestId('composer-context').getByText('Service: checkout-api'),
    ).toBeVisible();
    await page.keyboard.press('Escape');

    await expect(page.getByRole('button', { name: 'Tools' })).toHaveCount(0);
    await page.getByRole('button', { name: 'More' }).click();
    await expect(
      page.getByRole('combobox', { name: 'Agent mode' }),
    ).toContainText('Auto mode');
    await expect(page.getByRole('combobox', { name: 'Agent Profile' })).toBeVisible();
    await expect(page.getByRole('combobox', { name: 'Model' })).toBeVisible();
    await expect(page.getByRole('combobox', { name: 'Prompt' })).toBeVisible();
    await expect(page.getByText('Investigation limit')).toBeVisible();
    await page.keyboard.press('Escape');

    await page
      .getByRole('button', {
        name: /Why is web.*error rate increasing/,
      })
      .click();
    await expect(
      page.getByLabel(
        'Ask about incidents, services, or operational tasks…',
      ),
    ).toHaveValue(
      "Why is web's error rate increasing?",
    );
    await expect(
      page.getByRole('button', {
        name: /Create a service health dashboard for the last hour/,
      }),
    ).toBeVisible();
    await expect(
      page.getByTestId('conversation-context').getByText('web', { exact: true }),
    ).toBeVisible();
    await page.getByRole('button', { name: /More/ }).click();
    await expect(
      page.getByRole('combobox', { name: 'Agent mode' }),
    ).toContainText('Deep investigation');

    await page.keyboard.press('Escape');
    await page
      .getByRole('button', {
        name: /Create a service health dashboard for the last hour/,
      })
      .click();
    await expect(
      page.getByLabel(
        'Ask about incidents, services, or operational tasks…',
      ),
    ).toHaveValue('Create a service health dashboard for the last hour');
    await expect(page.getByRole('combobox', { name: 'Time' })).toContainText(
      'Last 1h',
    );
    await expect(
      page.getByRole('combobox', { name: 'Execution' }),
    ).toContainText('Execute with approval');
    await page.getByRole('button', { name: 'More' }).click();
    await expect(
      page.getByRole('combobox', { name: 'Agent mode' }),
    ).toContainText('Auto mode');
  });

  test('renders product evidence and regenerates an answer without duplicating the question', async ({
    page,
  }) => {
    await page.goto('/agent/chat');

    const initialComposerBox = await page
      .getByTestId('composer-shell')
      .boundingBox();
    await page
      .getByRole('button', {
        name: /Show current unacknowledged alerts/,
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
    const conversationTitle = page
      .getByTestId('conversation-history-title')
      .first();
    const conversationTime = page
      .getByTestId('conversation-history-time')
      .first();
    await expect(conversationTitle).toBeVisible();
    await expect(conversationTime).toBeVisible();
    const conversationTitleBox = await conversationTitle.boundingBox();
    const conversationTimeBox = await conversationTime.boundingBox();
    expect(conversationTitleBox).not.toBeNull();
    expect(conversationTimeBox).not.toBeNull();
    expect(
      Math.abs(
        (conversationTitleBox?.y ?? 0) +
          (conversationTitleBox?.height ?? 0) / 2 -
          ((conversationTimeBox?.y ?? 0) +
            (conversationTimeBox?.height ?? 0) / 2),
      ),
    ).toBeLessThan(3);
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
    await expect(page.getByText('List streams', { exact: true })).toBeVisible();
    await expect(page.getByText('list_streams')).not.toBeVisible();
    await page.getByText('Technical detail').click();
    await expect(page.getByText('list_streams')).toBeVisible();

    await page.getByRole('button', { name: 'Regenerate' }).click();
    await expect(page.getByText('Answer 2 / 2')).toBeVisible();
    await expect(page.locator('[data-message-role="user"]')).toHaveCount(1);
  });

  test('does not invent a service name when no service errors are observed', async ({
    page,
  }) => {
    await page.route('**/api/v1/web/topology**', (route) =>
      route.fulfill({ json: { nodes: [], edges: [] } }),
    );
    await page.route('**/api/v1/streams**', (route) =>
      route.fulfill({ json: [] }),
    );
    await page.goto('/agent/chat');

    const genericStarter = page.getByRole('button', {
      name: 'Which service has the highest error rate?',
    });
    await expect(genericStarter).toBeVisible();
    await expect(page.getByText('checkout-api', { exact: false })).toHaveCount(0);

    await genericStarter.click();
    await expect(
      page.getByLabel('Ask about incidents, services, or operational tasks…'),
    ).toHaveValue('Which service has the highest error rate?');
    await expect(page.getByTestId('composer-context')).toHaveCount(0);
  });

  test('uses an in-app confirmation dialog when deleting chat history', async ({
    page,
  }) => {
    let nativeDialogCount = 0;
    page.on('dialog', async (dialog) => {
      nativeDialogCount += 1;
      await dialog.dismiss();
    });

    await page.goto('/agent/chat');
    await page
      .getByRole('button', {
        name: /Show current unacknowledged alerts/,
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
      confirmDialog.getByText(/will be archived before it is removed/),
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

  test('keeps operational status compact and uses rounded active tabs', async ({
    page,
  }) => {
    await page.goto('/agent/chat');
    const agentHeader = page.getByTestId('agent-module-header');
    await expect(agentHeader.getByText('investigating')).toBeVisible();
    await expect(agentHeader.getByText('awaiting approval')).toBeVisible();
    await expect(agentHeader.getByText('Running automations')).toHaveCount(0);
    await expect(agentHeader.getByRole('link', { name: 'Chat' })).toBeVisible();

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
    const activeTab = page
      .getByTestId('alerts-subnav')
      .locator('a[href="/alerts/incidents"]');

    await expect(activeTab).toHaveClass(/rounded-md/);
    await expect(activeTab).toHaveClass(/bg-transparent/);
    await expect(activeTab).toHaveClass(/after:bg-indigo/);
    await expect(activeTab).not.toHaveClass(/border-b-2/);
  });

  test('keeps history visible and exposes context at the minimum supported width', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1024, height: 768 });
    await page.goto('/agent/chat');

    await expect(page.getByTestId('conversation-history')).toBeVisible();
    await page.getByRole('button', { name: 'Context', exact: true }).click();
    const contextDrawer = page.getByRole('dialog');
    await expect(contextDrawer).toContainText('Scope');
    await expect(contextDrawer).toContainText('Signals');
  });
});
