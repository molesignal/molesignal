import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Mole Agent pages', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('a tab opens with what it is for, not with its name again', async ({ page }) => {
    await page.goto('/agent/investigations');

    const surface = page.locator('[data-agent-content-surface]');
    const head = surface.locator('[data-agent-page-head]');
    // One line: what the tab is for, and the one thing that can be added to it.
    await expect(head).toContainText('Investigations from incidents or by hand');
    await expect(head.getByRole('button', { name: 'Create investigation' })).toBeVisible();
    expect((await head.boundingBox())!.height).toBeLessThanOrEqual(48);
    // The tabs name the page: its own name is kept for the outline, not drawn a second time.
    const title = surface.getByRole('heading', { level: 2, name: 'Investigations' });
    await expect(title).toHaveClass(/sr-only/);

    // The table is the tab's content: it reaches the edges of the surface, with the gutter the
    // line over it has, and is not a bordered card set into the surface.
    const list = surface.locator('[data-agent-list]');
    const [surfaceBox, listBox] = await Promise.all([surface.boundingBox(), list.boundingBox()]);
    expect(listBox!.x).toBe(surfaceBox!.x);
    expect(listBox!.width).toBe(surfaceBox!.width);
    await expect(list).toHaveCSS('border-top-width', '0px');
    const gutters = await surface.evaluate((element) => {
      const left = (selector: string) => {
        const target = element.querySelector(selector)!;
        const range = document.createRange();
        range.selectNodeContents(target);
        return Math.round(range.getBoundingClientRect().left);
      };
      return [left('[data-agent-page-head] p'), left('[data-agent-list] th')];
    });
    expect(gutters[0]).toBe(gutters[1]);
  });

  test('a tab with nothing in it yet says so once, with its one action', async ({ page }) => {
    await page.route(/\/api\/v1\/agent\/investigations(?:\?.*)?$/, (route) =>
      route.fulfill({ json: { investigations: [] } }),
    );
    await page.goto('/agent/investigations');

    const surface = page.locator('[data-agent-content-surface]');
    await expect(surface.getByText('No investigations yet')).toBeVisible();
    // What the tab is for and how its first entry comes about, in the empty state itself.
    await expect(surface).toContainText('Investigations from incidents or by hand');
    await expect(surface).toContainText('Turn complex or long-running questions into tracked investigations.');
    // The action stood twice: over the empty state and in it.
    await expect(surface.getByRole('button', { name: 'Create investigation' })).toHaveCount(1);
    await expect(surface.locator('[data-agent-page-head]')).toHaveCount(0);
  });

  test('the sections of the settings are a row of the navigation', async ({ page }) => {
    await page.goto('/agent/settings/tools');

    const header = page.getByTestId('agent-module-header');
    const sections = header.locator('[data-agent-subnavigation="settings"]');
    const names = [
      'Agent profiles',
      'Models & reasoning',
      'Usage',
      'Prompt management',
      'Skills',
      'Tools & capabilities',
      'Inbound MCP',
      'Data access',
      'Network policy',
      'Approval policy',
    ];
    await expect(sections.getByRole('link')).toHaveCount(names.length);
    for (const name of names) {
      await expect(sections.getByRole('link', { name, exact: true })).toBeVisible();
    }
    await expect(sections.locator('[aria-current="page"]')).toHaveAccessibleName('Tools & capabilities');

    // The page is the open section: no "Agent settings" band over it and no second switch in it.
    const surface = page.locator('[data-agent-content-surface]');
    await expect(surface.getByRole('tablist')).toHaveCount(0);
    await expect(surface.getByRole('heading', { level: 2, name: 'Agent settings' })).toHaveClass(/sr-only/);
    // The groups of tools start in the upper half of the surface, not under three bands of
    // headers, and are folded until they are opened: a group is found by its name and count.
    const groups = surface.locator('[data-agent-tool-group]');
    await expect(groups.first()).toBeVisible();
    expect((await groups.first().boundingBox())!.y).toBeLessThan(360);
    expect(await groups.evaluateAll((all) => all.every((group) => group.getAttribute('aria-expanded') === 'false'))).toBe(true);
    await expect(surface.getByText('get_trace', { exact: true })).toHaveCount(0);

    await groups.first().click();
    await expect(groups.first()).toHaveAttribute('aria-expanded', 'true');
    await expect(surface.getByText('get_trace', { exact: true })).toBeVisible();
    // What was opened stays open on the next visit.
    await page.reload();
    await expect(surface.locator('[data-agent-tool-group]').first()).toHaveAttribute('aria-expanded', 'true');
    await surface.locator('[data-agent-tool-group]').first().click();

    // A search shows its matches whichever group they are in.
    await surface.getByLabel(/search tools/i).fill('query_logs');
    await expect(surface.getByText('query_logs', { exact: true })).toBeVisible();
    await surface.getByLabel(/search tools/i).fill('');
    await expect(surface.getByText('query_logs', { exact: true })).toHaveCount(0);

    await sections.getByRole('link', { name: 'Agent profiles' }).click();
    await expect(page).toHaveURL(/\/agent\/settings$/);
    await expect(sections.locator('[aria-current="page"]')).toHaveAccessibleName('Agent profiles');

    // Outside the settings the navigation has no second row.
    await page.goto('/agent/approvals');
    await expect(header.locator('[data-agent-subnavigation]')).toHaveCount(0);
  });

  test('a memory reads in one rhythm and names the chat it came from', async ({ page }) => {
    await page.goto('/agent/knowledge/memory');

    const row = page.locator('[data-agent-memory-row="Payments database"]');
    await row.locator('summary').click();

    // The chat it came from is named by its title, and leads to it.
    const source = row.locator('[data-agent-memory-source]');
    await expect(source).toHaveText('Payments latency in eu-west-1');
    await expect(source).toHaveAttribute('href', '/agent/chat?chat=chat-payments-latency');
    // A memory written outside a chat names none.
    await page.locator('[data-agent-memory-row="Time zone"] summary').click();
    await expect(
      page.locator('[data-agent-memory-row="Time zone"] [data-agent-memory-source]'),
    ).toHaveCount(0);

    // Name, when it applies, the memory and its date: the same room between each two lines.
    const gaps = await row.evaluate((element) => {
      const lines = [
        element.querySelector('summary > span:first-child')!,
        element.querySelector('summary > span:last-child')!,
        element.querySelector('details > p:first-of-type')!,
        element.querySelector('details > p:last-of-type')!,
      ].map((line) => line.getBoundingClientRect());
      return lines.slice(1).map((line, index) => Math.round(line.top - lines[index]!.bottom));
    });
    expect(gaps[0]).toBeGreaterThan(0);
    expect(new Set(gaps).size).toBe(1);
  });

  test('prompts say how many there are in one line, and a built-in one shows no date of 1970', async ({
    page,
  }) => {
    await page.goto('/agent/settings/prompts');

    const surface = page.locator('[data-agent-content-surface]');
    const facts = surface.locator('[data-facts]');
    await expect(facts.locator('[data-fact="total"]')).toHaveText(/^Total prompts\s*\d+$/);
    expect((await facts.boundingBox())!.height).toBeLessThanOrEqual(32);
    await expect(surface).not.toContainText('1970-01-01');
  });
});

test('the Mole Agent chat says the time range in the language of the product', async ({
  page,
  mockServer,
}) => {
  await page.addInitScript(() => {
    localStorage.setItem('molesignal-ui-prefs', JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }));
  });
  await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
  await page.goto('/agent/chat');

  // "Last 1h" stood in the context panel, in English on a page written in Chinese.
  await expect(page.getByText('时间范围')).toBeVisible();
  await expect(page.getByText('Last 1h')).toHaveCount(0);
  await expect(page.getByText('近 1 小时').first()).toBeVisible();
});

test.describe('Mole Agent chat without a model', () => {
  for (const [role, allowed] of [
    ['one who may set a model up', true],
    ['one who may not', false],
  ] as const) {
    test(`says that nothing can answer yet, to ${role}`, async ({ page, mockServer }) => {
      await mountMockRoutes(
        page,
        mockServer.port,
        allowed ? {} : { role: 'viewer', capabilityPermissions: ['agent.use', 'streams.query'] },
      );
      await page.route(/\/api\/v1\/agent\/settings\/model-providers(?:\?.*)?$/, (route) =>
        route.fulfill({ json: [] }),
      );
      await page.goto('/agent/chat');

      const notice = page.locator('[data-agent-no-model]');
      await expect(notice).toContainText('No AI model is enabled');
      const configure = notice.getByRole('link', { name: 'Set up an AI model' });
      if (allowed) {
        await expect(configure).toHaveAttribute('href', '/agent/settings/models');
      } else {
        await expect(configure).toHaveCount(0);
        await expect(notice).toContainText('Ask an administrator');
      }
    });
  }

  test('says nothing of it while a model is enabled', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/agent/chat');

    await expect(page.getByTestId('composer-shell')).toBeVisible();
    await expect(page.locator('[data-agent-no-model]')).toHaveCount(0);
  });
});

