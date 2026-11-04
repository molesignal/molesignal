import type { Page } from '@playwright/test';

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
      'IM',
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

test.describe('Mole Agent settings ask for what the API of each section asks for', () => {
  const MODEL_MANAGER = ['agent.use', 'agent.models.read', 'agent.models.manage'];
  const PROVIDER = '/api/v1/agent/settings/model-providers/openai-gpt-4o';

  const openSection = async (
    page: Page,
    port: number,
    section: string,
    capabilityPermissions: string[],
  ) => {
    await mountMockRoutes(page, port, { role: 'viewer', capabilityPermissions });
    await page.goto(`/agent/settings/${section}`);
  };
  const openModels = (page: Page, port: number, capabilityPermissions: string[]) =>
    openSection(page, port, 'models', capabilityPermissions);
  const surface = (page: Page) => page.locator('[data-agent-content-surface]');
  const addProvider = (page: Page) => surface(page).getByRole('button', { name: 'Add model provider' });
  const editProvider = (page: Page) =>
    surface(page).getByRole('button', { name: 'Edit model provider' }).first();
  const notice = (page: Page) => surface(page).getByRole('status').filter({ hasText: /^Requires the/ });

  /** The calls made to matching paths. The mock answers anything, so a refusal shows as a call made. */
  function callsTo(page: Page, path: RegExp): string[] {
    const calls: string[] = [];
    page.on('request', (request) => {
      const { pathname } = new URL(request.url());
      if (path.test(pathname)) calls.push(`${request.method()} ${pathname}`);
    });
    return calls;
  }

  test('the models permissions alone add a provider and ask for nothing they would be refused', async ({
    page,
    mockServer,
  }) => {
    const refused = callsTo(page, /\/agent\/(tools|settings\/(agent-profiles|currencies))$/);
    await openModels(page, mockServer.port, MODEL_MANAGER);

    await addProvider(page).click();
    const drawer = page.getByRole('dialog', { name: 'Add model provider' });
    await drawer.getByLabel('Display name').fill('Gateway');
    await drawer.getByLabel('Default model').fill('gpt-5');
    await drawer.getByLabel('API key').fill('sk-gateway');
    // The currencies are the usage permissions' to read, so the provider keeps the default one.
    await expect(drawer.getByRole('combobox', { name: 'Currency' })).toBeDisabled();
    const created = page.waitForRequest(
      (request) =>
        request.method() === 'POST' &&
        new URL(request.url()).pathname === '/api/v1/agent/settings/model-providers',
    );
    await drawer.getByRole('button', { name: 'Create' }).click();

    expect((await created).postDataJSON()).toMatchObject({
      name: 'Gateway',
      defaultModel: 'gpt-5',
      priceCurrency: 'USD',
      apiKey: 'sk-gateway',
    });
    await expect(surface(page).getByText('Gateway', { exact: true })).toBeVisible();
    await expect(notice(page)).toHaveCount(0);
    await expect(surface(page).getByText('Currencies', { exact: true })).toHaveCount(0);
    expect(refused).toEqual([]);
  });

  test('the permission of another section does not open it, and the notice names the right one', async ({
    page,
    mockServer,
  }) => {
    await openModels(page, mockServer.port, ['agent.use', 'agent.models.read', 'agent.profiles.manage']);

    await expect(notice(page)).toHaveText('Requires the agent.models.manage permission.');
    await expect(addProvider(page)).toBeDisabled();
    await expect(editProvider(page)).toBeDisabled();
  });

  test('the models permission does not open the other sections', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, {
      role: 'viewer',
      capabilityPermissions: [...MODEL_MANAGER, 'agent.profiles.read'],
    });
    await page.goto('/agent/settings');

    await expect(notice(page)).toHaveText('Requires the agent.profiles.manage permission.');
    await expect(surface(page).getByRole('button', { name: 'Create profile' })).toBeDisabled();
  });

  test('replacing the key of a provider takes a permission of its own', async ({ page, mockServer }) => {
    await openModels(page, mockServer.port, MODEL_MANAGER);
    const writes = callsTo(page, /\/agent\/settings\/model-providers\/.+/);

    await editProvider(page).click();
    const drawer = page.getByRole('dialog', { name: 'Edit model provider' });
    await expect(drawer.getByLabel('API key')).toBeDisabled();
    await expect(drawer).toContainText('Requires the agent.models.rotate permission.');
    await drawer.getByLabel('Display name').fill('OpenAI renamed');
    await drawer.getByRole('button', { name: 'Save' }).click();

    await expect(surface(page).getByText('OpenAI renamed', { exact: true })).toBeVisible();
    expect(writes).toEqual([`PUT ${PROVIDER}`]);
  });

  test('that permission alone replaces the key and changes nothing else', async ({ page, mockServer }) => {
    await openModels(page, mockServer.port, ['agent.use', 'agent.models.read', 'agent.models.rotate']);
    const writes = callsTo(page, /\/agent\/settings\/model-providers\/.+/);

    await expect(notice(page)).toHaveText('Requires the agent.models.manage permission.');
    await expect(addProvider(page)).toBeDisabled();
    await editProvider(page).click();
    const drawer = page.getByRole('dialog', { name: 'Edit model provider' });
    await expect(drawer).toContainText('Requires the agent.models.manage permission.');
    await expect(drawer.getByLabel('Display name')).toBeDisabled();
    await expect(drawer.getByRole('combobox', { name: 'Provider type' })).toBeDisabled();
    await expect(drawer.getByRole('combobox', { name: 'Currency' })).toBeDisabled();
    await expect(drawer.getByRole('switch', { name: 'Enable model provider' })).toBeDisabled();
    // A save with no key would have nothing it may send.
    const save = drawer.getByRole('button', { name: 'Save' });
    await expect(save).toBeDisabled();

    await drawer.getByLabel('API key').fill('sk-rotated');
    await save.click();

    await expect(page.getByRole('dialog')).toHaveCount(0);
    expect(writes).toEqual([`POST ${PROVIDER}/rotate-key`]);
  });

  test('the currencies answer to the usage permissions, whatever locks the providers', async ({
    page,
    mockServer,
  }) => {
    await openModels(page, mockServer.port, [
      'agent.use',
      'agent.models.read',
      'agent.usage.read',
      'agent.usage.manage',
    ]);

    await expect(addProvider(page)).toBeDisabled();
    await surface(page).getByRole('button', { name: 'Add currency' }).click();
    await expect(page.getByRole('dialog', { name: 'Add currency' })).toBeVisible();
  });

  test('reading the currencies offers them to price in, and does not change them', async ({
    page,
    mockServer,
  }) => {
    await openModels(page, mockServer.port, [...MODEL_MANAGER, 'agent.usage.read']);

    await expect(surface(page).locator('[data-agent-currency="CNY"]')).toBeVisible();
    await expect(surface(page).getByRole('button', { name: 'Add currency' })).toHaveCount(0);

    await addProvider(page).click();
    const drawer = page.getByRole('dialog', { name: 'Add model provider' });
    await drawer.getByRole('combobox', { name: 'Currency' }).click();
    await expect(page.getByRole('option', { name: /^CNY/ })).toBeVisible();
  });

  for (const { section, read, manage, control } of [
    {
      section: 'usage',
      read: 'agent.usage.read',
      manage: 'agent.usage.manage',
      control: (page: Page) => surface(page).getByRole('button', { name: 'Save budget' }),
    },
    {
      section: 'prompts',
      read: 'agent.prompts.read',
      manage: 'agent.prompts.manage',
      control: (page: Page) => surface(page).getByRole('button', { name: 'Create prompt' }),
    },
    {
      section: 'inbound-mcp',
      read: 'agent.inbound_mcp.read',
      manage: 'agent.inbound_mcp.manage',
      control: (page: Page) => surface(page).getByRole('button', { name: 'Save settings' }),
    },
    {
      section: 'im',
      read: 'agent.im.read',
      manage: 'agent.im.manage',
      control: (page: Page) => surface(page).getByRole('switch', { name: 'Answer in IM with Ops Feishu' }),
    },
  ]) {
    test(`${section} opens on its own permissions and asks for nothing they would be refused`, async ({
      page,
      mockServer,
    }) => {
      const refused = callsTo(page, /\/agent\/(tools|settings\/(agent-profiles|model-providers))$/);
      await openSection(page, mockServer.port, section, ['agent.use', read, manage]);

      await expect(control(page)).toBeEnabled();
      await expect(notice(page)).toHaveCount(0);
      expect(refused).toEqual([]);
    });

    test(`${section} stays locked on the permission of another section, and names its own`, async ({
      page,
      mockServer,
    }) => {
      await openSection(page, mockServer.port, section, ['agent.use', read, 'agent.profiles.manage']);

      await expect(control(page)).toBeDisabled();
      await expect(notice(page)).toHaveText(`Requires the ${manage} permission.`);
    });
  }

  for (const [section, edit] of [
    ['data', 'Edit data access'],
    ['approvals', 'Edit approval policy'],
  ] as const) {
    test(`${section} stays on the permission that changes a profile`, async ({ page, mockServer }) => {
      await openSection(page, mockServer.port, section, ['agent.use', 'agent.profiles.read', 'agent.usage.manage']);

      await expect(notice(page)).toHaveText('Requires the agent.profiles.manage permission.');
      await expect(surface(page).getByRole('button', { name: edit })).toBeDisabled();
    });
  }

  test('the usage permissions alone save a budget', async ({ page, mockServer }) => {
    await openSection(page, mockServer.port, 'usage', ['agent.use', 'agent.usage.read', 'agent.usage.manage']);

    const saved = page.waitForRequest(
      (request) =>
        request.method() === 'PUT' && new URL(request.url()).pathname === '/api/v1/agent/usage/budget',
    );
    await surface(page).getByRole('button', { name: 'Save budget' }).click();
    await saved;
    await expect(page.getByText('Budget saved')).toBeVisible();
  });

  test('usage shows what was used and what the budget is to one who may not change it', async ({
    page,
    mockServer,
  }) => {
    await openSection(page, mockServer.port, 'usage', ['agent.use', 'agent.usage.read']);

    await expect(surface(page).locator('[data-agent-usage-today]')).toBeVisible();
    const form = surface(page).locator('[data-agent-usage-budget]');
    await expect(form.getByRole('combobox', { name: 'Daily token limit' })).toBeDisabled();
    await expect(
      form.getByRole('combobox', { name: 'When the budget is used up, chats and on-demand analyses' }),
    ).toBeDisabled();
  });

  test('prompts can be searched and read without the permission to change them', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      role: 'viewer',
      capabilityPermissions: ['agent.use', 'agent.prompts.read'],
    });
    await page.route('**/api/v1/agent/settings/prompts', (route) =>
      route.fulfill({
        json: [
          {
            id: 'prompt-builtin',
            orgId: null,
            userId: null,
            scope: 'builtin',
            builtinKey: 'system.default',
            purpose: 'system',
            name: 'Built-in instruction',
            body: 'Answer from the evidence.',
            variablesSchema: { type: 'object', properties: {} },
            isDefault: true,
            enabled: true,
            version: 1,
            parentId: null,
            createdAt: 0,
            updatedAt: 0,
          },
          {
            id: 'prompt-own',
            orgId: 'org',
            userId: null,
            scope: 'org',
            builtinKey: 'system.default',
            purpose: 'system',
            name: 'Our instruction',
            body: 'Answer briefly.',
            variablesSchema: { type: 'object', properties: {} },
            isDefault: false,
            enabled: true,
            version: 2,
            parentId: 'prompt-builtin',
            createdAt: 1,
            updatedAt: 1,
          },
        ],
      }),
    );
    await page.goto('/agent/settings/prompts');

    const row = (name: string) => surface(page).locator('article').filter({ hasText: name });
    await expect(row('Built-in instruction').getByRole('button', { name: 'Create override' })).toBeDisabled();
    for (const action of ['Edit', 'Set as default', 'Restore', 'Delete']) {
      await expect(row('Our instruction').getByRole('button', { name: action })).toBeDisabled();
    }

    await surface(page).getByLabel('Search prompts').fill('briefly');
    await expect(row('Built-in instruction')).toHaveCount(0);
    await row('Our instruction').getByRole('button', { name: 'View' }).click();
    await expect(page.getByRole('dialog', { name: 'View prompt' })).toBeVisible();
  });

  for (const [who, permissions, locked] of [
    ['one who may not change it', ['agent.use', 'agent.inbound_mcp.read'], true],
    ['one who may', ['agent.use', 'agent.inbound_mcp.read', 'agent.inbound_mcp.manage'], false],
  ] as const) {
    test(`inbound MCP keeps its addresses and prompt to copy, and its settings for ${who}`, async ({
      page,
      mockServer,
    }) => {
      await mountMockRoutes(page, mockServer.port, { role: 'viewer', capabilityPermissions: [...permissions] });
      await page.route('**/api/v1/agent/settings/inbound-mcp/oauth-connections', (route) =>
        route.fulfill({
          json: {
            connections: [
              {
                familyId: 'family-1',
                clientId: 'mspat-client1',
                clientName: 'An agent tool',
                scope: 'mcp',
                userDisplayName: 'Dev',
                active: true,
                lastExpiresAt: 4_000_000_000_000_000,
              },
            ],
          },
        }),
      );
      await page.goto('/agent/settings/inbound-mcp');

      await expect(surface(page).getByRole('button', { name: 'Copy address' }).first()).toBeEnabled();
      await expect(surface(page).getByRole('button', { name: 'Copy prompt' })).toBeEnabled();
      const settings = [
        surface(page).getByRole('switch', { name: 'Enabled' }),
        surface(page).getByLabel('Allowed browser origins'),
        surface(page).getByRole('spinbutton').first(),
        surface(page).getByRole('button', { name: 'Revoke' }),
      ];
      for (const control of settings) {
        if (locked) await expect(control).toBeDisabled();
        else await expect(control).toBeEnabled();
      }
    });
  }

  test('IM keeps a callback address to copy without the permission to change a bot', async ({
    page,
    mockServer,
  }) => {
    await openSection(page, mockServer.port, 'im', ['agent.use', 'agent.im.read']);

    await expect(
      surface(page).getByRole('combobox', { name: 'Where new group threads of Ops Feishu go' }),
    ).toBeDisabled();
    await expect(surface(page).getByRole('button', { name: 'Copy the callback address' })).toBeEnabled();
  });

  test('the network permissions alone change what members may reach, and nothing of a profile shows', async ({
    page,
    mockServer,
  }) => {
    const refused = callsTo(page, /\/agent\/(tools|settings\/(agent-profiles|model-providers))$/);
    await openSection(page, mockServer.port, 'network', [
      'agent.use',
      'agent.network_policy.read',
      'agent.network_policy.manage',
    ]);

    const policy = surface(page).locator('[data-agent-member-network]');
    await policy.getByLabel('Internal hosts').fill('git.internal.example');
    const saved = page.waitForRequest(
      (request) =>
        request.method() === 'PUT' && new URL(request.url()).pathname === '/api/v1/agent/network-policy',
    );
    await policy.getByRole('button', { name: 'Save' }).click();

    expect((await saved).postDataJSON()).toMatchObject({ privateHosts: ['git.internal.example'] });
    await expect(notice(page)).toHaveCount(0);
    await expect(surface(page).getByText('Network access', { exact: true })).toHaveCount(0);
    await expect(surface(page).getByText('Runtime capabilities', { exact: true })).toHaveCount(0);
    await expect(surface(page).getByRole('button', { name: 'Edit network policy' })).toHaveCount(0);
    expect(refused).toEqual([]);
  });

  test('the profiles permissions change the network of a profile, and do not read what members may reach', async ({
    page,
    mockServer,
  }) => {
    const refused = callsTo(page, /\/agent\/network-policy$/);
    await openSection(page, mockServer.port, 'network', [
      'agent.use',
      'agent.profiles.read',
      'agent.profiles.manage',
      'agent.tools.read',
    ]);

    await expect(surface(page).getByRole('button', { name: 'Edit network policy' })).toBeEnabled();
    await expect(surface(page).getByText('Network access', { exact: true })).toBeVisible();
    await expect(surface(page).getByText('Runtime capabilities', { exact: true })).toBeVisible();
    const policy = surface(page).locator('[data-agent-member-network]');
    await expect(policy.getByRole('status')).toHaveText('Requires the agent.network_policy.read permission.');
    await expect(policy.getByLabel('Internal hosts')).toHaveCount(0);
    expect(refused).toEqual([]);
  });

  test('each part of the network section says which permission it lacks', async ({ page, mockServer }) => {
    await openSection(page, mockServer.port, 'network', [
      'agent.use',
      'agent.profiles.read',
      'agent.network_policy.read',
    ]);

    await expect(surface(page).getByRole('button', { name: 'Edit network policy' })).toBeDisabled();
    await expect(notice(page).filter({ hasText: 'agent.profiles.manage' })).toBeVisible();
    const policy = surface(page).locator('[data-agent-member-network]');
    await expect(policy.getByRole('status')).toHaveText('Requires the agent.network_policy.manage permission.');
    const hosts = policy.getByLabel('Internal hosts');
    await expect(hosts).toBeDisabled();
    await expect(policy.getByRole('button', { name: 'Save' })).toBeDisabled();
    // A locked field keeps the room it has when it can be used.
    const room = await hosts.locator('xpath=ancestor::label').boundingBox();
    expect((await hosts.boundingBox())!.width).toBeCloseTo(room!.width, 0);
  });

  for (const [section, read] of [
    ['profiles', 'agent.profiles.read'],
    ['data', 'agent.profiles.read'],
    ['approvals', 'agent.profiles.read'],
    ['usage', 'agent.usage.read'],
    ['prompts', 'agent.prompts.read'],
    ['inbound-mcp', 'agent.inbound_mcp.read'],
    ['im', 'agent.im.read'],
  ] as const) {
    test(`${section} says which permission opens it, and asks for nothing without it`, async ({
      page,
      mockServer,
    }) => {
      const refused = callsTo(page, /^\/api\/v1\/agent\/(settings\/|usage|im\/bots)/);
      // Its permission to change it is no way in: there would be nothing to change.
      const manage = read.replace('.read', '.manage');
      await openSection(page, mockServer.port, section, ['agent.use', manage]);

      const denied = surface(page).locator('[data-product-state]');
      await expect(denied).toContainText('Permission required');
      await expect(denied).toContainText(`Requires the ${read} permission.`);
      await expect(notice(page)).toHaveCount(0);
      expect(refused).toEqual([]);
    });
  }

  for (const section of ['data', 'approvals']) {
    test(`${section} shows no defaults while there is no profile to show`, async ({ page, mockServer }) => {
      await mountMockRoutes(page, mockServer.port, {
        role: 'viewer',
        capabilityPermissions: ['agent.use', 'agent.profiles.read'],
      });
      await page.route('**/api/v1/agent/settings/agent-profiles', (route) =>
        route.fulfill({ json: { profiles: [] } }),
      );
      await page.goto(`/agent/settings/${section}`);

      await expect(surface(page).locator('[data-product-state]')).toContainText('No agent profiles yet');

      await page.route('**/api/v1/agent/settings/agent-profiles', (route) =>
        route.fulfill({ status: 500, json: { message: 'profiles are unavailable' } }),
      );
      await page.reload();

      await expect(surface(page).getByRole('alert')).toContainText('profiles are unavailable');
    });
  }

  test('the usage permissions alone keep the currencies, with no word about a provider', async ({
    page,
    mockServer,
  }) => {
    const refused = callsTo(page, /\/agent\/settings\/model-providers$/);
    await openModels(page, mockServer.port, ['agent.use', 'agent.usage.read', 'agent.usage.manage']);

    await expect(surface(page).getByRole('button', { name: 'Add currency' })).toBeEnabled();
    await expect(addProvider(page)).toHaveCount(0);
    await expect(notice(page)).toHaveCount(0);
    expect(refused).toEqual([]);
  });

  test('the models section says what opens it to one who may read nothing in it', async ({
    page,
    mockServer,
  }) => {
    const refused = callsTo(page, /\/agent\/settings\/(model-providers|currencies)$/);
    await openModels(page, mockServer.port, ['agent.use', 'agent.models.manage']);

    const denied = surface(page).locator('[data-product-state]');
    await expect(denied).toContainText('Requires the agent.models.read permission.');
    await expect(notice(page)).toHaveCount(0);
    expect(refused).toEqual([]);
  });

  for (const [who, permissions, offered] of [
    ['one who may not open the connectors', [], false],
    ['one who may', ['notify.connectors.read'], true],
  ] as const) {
    test(`IM with no connector offers to add one only to ${who}`, async ({ page, mockServer }) => {
      await mountMockRoutes(page, mockServer.port, {
        role: 'viewer',
        capabilityPermissions: ['agent.use', 'agent.im.read', ...permissions],
      });
      await page.route('**/api/v1/agent/im/bots', (route) =>
        route.fulfill({ json: { bots: [], externalUrl: true } }),
      );
      await page.goto('/agent/settings/im');

      await expect(surface(page).getByText('No connector that can answer in IM yet')).toBeVisible();
      const add = surface(page).getByRole('link', { name: 'Add a connector' });
      if (offered) await expect(add).toHaveAttribute('href', '/settings/notify/connectors');
      else await expect(add).toHaveCount(0);
    });
  }

  test('inbound MCP links only to the credential pages the reader may open', async ({ page, mockServer }) => {
    await openSection(page, mockServer.port, 'inbound-mcp', ['agent.use', 'agent.inbound_mcp.read']);

    await expect(surface(page).getByText('Credentials', { exact: true })).toBeVisible();
    await expect(surface(page).getByRole('link', { name: 'Manage API tokens' })).toHaveCount(0);
    await expect(surface(page).getByRole('link', { name: 'Manage Service Accounts' })).toHaveCount(0);
  });

  test('inbound MCP links to the credential page the reader may open', async ({ page, mockServer }) => {
    await openSection(page, mockServer.port, 'inbound-mcp', [
      'agent.use',
      'agent.inbound_mcp.read',
      'api_tokens.read',
    ]);

    await expect(surface(page).getByRole('link', { name: 'Manage API tokens' })).toHaveAttribute(
      'href',
      '/iam/api-tokens',
    );
    await expect(surface(page).getByRole('link', { name: 'Manage Service Accounts' })).toHaveCount(0);
  });
});

