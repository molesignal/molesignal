import { Buffer } from 'node:buffer';

import type { Page, Request } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const skillRow = (page: Page, name: string) => page.locator(`[data-agent-skill="${name}"]`);

const isRequest = (method: string, path: RegExp) => (request: Request) =>
  request.method() === method && path.test(new URL(request.url()).pathname);

test.describe('Mole Agent skills, MCP servers and team chats', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('the skills section lists every source, says whose each skill is and shows what it holds', async ({
    page,
  }) => {
    await page.goto('/agent/settings/skills');
    const panel = page.locator('[data-agent-skills]');
    await expect(skillRow(page, 'incident-triage')).toContainText('Incident triage');
    await expect(skillRow(page, 'incident-triage')).toContainText('Built-in');
    await expect(skillRow(page, 'k8s-debug')).toContainText('Needs a Mole Runner');
    await expect(skillRow(page, 'runbook')).toContainText('Platform');
    await expect(skillRow(page, 'runbook')).toContainText('Repository');

    // The filters say how many each holds.
    await expect(panel.locator('[data-agent-skill-filter="instance"]')).toContainText('2');
    await panel.locator('[data-agent-skill-filter="team"]').click();
    await expect(panel.locator('[data-agent-skill]')).toHaveCount(1);
    await panel.locator('[data-agent-skill-filter="all"]').click();

    // Turning one off goes to the backend.
    const patch = page.waitForRequest(isRequest('PATCH', /\/api\/v1\/agent\/skills\/skill-postmortem$/));
    await skillRow(page, 'postmortem').getByRole('switch').click();
    expect((await patch).postDataJSON()).toEqual({ enabled: false });

    // What it holds: instructions, files, and a text file read on the spot.
    await skillRow(page, 'postmortem').getByRole('button', { name: /postmortem/ }).first().click();
    const drawer = page.getByRole('dialog');
    await expect(drawer).toContainText('/postmortem');
    await expect(drawer).toContainText('get_incident, query_logs');
    await drawer.getByRole('button', { name: /templates\/report\.md/ }).click();
    await expect(drawer).toContainText('Contents of templates/report.md');
  });

  test('an upload with a name already in the scope replaces it only when asked again', async ({
    page,
  }) => {
    await page.goto('/agent/settings/skills');
    await page.getByRole('button', { name: 'Upload skill' }).first().click();
    const drawer = page.getByRole('dialog');
    await drawer.getByLabel('Package').setInputFiles({
      name: 'postmortem.zip',
      mimeType: 'application/zip',
      buffer: Buffer.from('PK'),
    });
    await drawer.getByRole('button', { name: 'Upload', exact: true }).click();
    await expect(drawer.getByRole('alert')).toContainText('already has a skill called postmortem');

    const upload = page.waitForRequest(isRequest('POST', /\/api\/v1\/agent\/skills$/));
    await drawer.getByRole('button', { name: 'Replace the skill' }).click();
    expect((await upload).postData() ?? '').toContain('name="replace"');
    await expect(page.getByText('Skill uploaded')).toBeVisible();
  });

  test('a repository is added for a scope and says how it synchronized', async ({ page }) => {
    await page.goto('/agent/settings/skills');
    await expect(page.locator('[data-agent-skill-repository="partial"]')).toContainText(
      'SKILL.md needs a `description`',
    );

    await page.getByRole('button', { name: 'Add repository' }).first().click();
    const drawer = page.getByRole('dialog');
    await drawer.getByLabel('Repository address').fill('https://github.com/acme/sre-skills');
    await drawer.getByLabel('Folder').fill('skills');
    const create = page.waitForRequest(isRequest('POST', /\/api\/v1\/agent\/skill-repositories$/));
    await drawer.getByRole('button', { name: 'Save' }).click();
    const body = (await create).postDataJSON() as { scope: string; url: string; dir: string };
    expect(body).toMatchObject({ scope: 'personal', url: 'https://github.com/acme/sre-skills', dir: 'skills' });
    await expect(page.getByText('Synchronized: 2 added, 0 updated, 0 removed.')).toBeVisible();
    await expect(page.locator('[data-agent-skill-repository]')).toHaveCount(2);
  });

  test('a member saves their own credential for a server that asks each member for one', async ({
    page,
  }) => {
    await page.goto('/agent/settings/tools');
    await page.locator('[data-agent-mcp-servers]').click();
    const linear = page.locator('[data-agent-mcp-server="linear"]');
    await expect(linear).toContainText('Platform');
    await expect(linear).toContainText('SSE (legacy)');

    await linear.locator('[data-agent-mcp-my-credential="missing"]').click();
    const drawer = page.getByRole('dialog', { name: /My credential for Linear/ });
    await expect(drawer.locator('[data-agent-mcp-instructions]')).toContainText(
      'Linear → Settings → API',
    );
    const save = page.waitForRequest(
      isRequest('PUT', /\/api\/v1\/agent\/mcp-servers\/mcp-linear\/my-credential$/),
    );
    await drawer.getByLabel('Credential').fill('lin_api_secret_abcd');
    await drawer.getByRole('button', { name: 'Save' }).click();
    expect((await save).postDataJSON()).toEqual({ value: 'lin_api_secret_abcd' });
    await expect(linear.locator('[data-agent-mcp-my-credential="saved"]')).toContainText('••••abcd');

    // Its prompts are listed as the commands they run as.
    await linear.getByRole('button', { name: 'Tools and prompts' }).click();
    await expect(page.locator('[data-agent-mcp-details="linear"]')).toContainText('/linear:triage');
  });

  test('a pasted client configuration makes servers and says what is still to fill in', async ({
    page,
  }) => {
    await page.goto('/agent/settings/tools');
    await page.locator('[data-agent-mcp-servers]').click();
    await page.getByRole('button', { name: 'Import' }).click();
    const drawer = page.getByRole('dialog', { name: 'Import MCP configuration' });
    await drawer.getByLabel('Configuration JSON').fill(
      JSON.stringify({
        mcpServers: {
          sentry: { url: 'https://mcp.sentry.dev/mcp', headers: { Authorization: 'Bearer ${input:token}' } },
          filesystem: { command: 'npx', args: ['-y', '@modelcontextprotocol/server-filesystem'] },
        },
      }),
    );
    await drawer.getByRole('button', { name: 'Import', exact: true }).click();
    const report = drawer.locator('[data-agent-mcp-import-report]');
    await expect(report).toContainText('sentry');
    await expect(report).toContainText('Still to fill in: Authorization');
    await expect(report).toContainText('A stdio server, started by MoleSignal');
  });

  test('the editor keeps a saved secret header hidden and shows its last characters', async ({
    page,
  }) => {
    await page.goto('/agent/settings/tools');
    await page.locator('[data-agent-mcp-servers]').click();
    const server = page.locator('[data-agent-mcp-server="internal_observability"]');
    await server.getByRole('button', { name: 'Actions' }).click();
    await page.getByRole('menuitem', { name: 'Edit' }).click();
    const headers = page.locator('[data-agent-mcp-values="header"]');
    await expect(headers.getByLabel('Header name').nth(0)).toHaveValue('x-tenant');
    await expect(headers.getByLabel('Value').nth(0)).toHaveValue('acme');
    await expect(headers.getByLabel('Value').nth(1)).toHaveAttribute('type', 'password');
    await expect(headers.getByLabel('Value').nth(1)).toHaveAttribute('placeholder', /9c1d/);
  });

  test('a new chat can be for a team, chosen in the composer or with /team', async ({ page }) => {
    await page.goto('/agent/chat');
    const controls = page.getByTestId('composer-controls');
    await controls.getByRole('combobox', { name: 'Team' }).click();
    await page.getByRole('option', { name: 'Platform' }).click();

    const create = page.waitForRequest(isRequest('POST', /\/api\/v1\/agent\/chat$/));
    const box = page.getByTestId('composer-input');
    await box.fill('Why is checkout slow?');
    await box.press('Enter');
    expect((await create).postDataJSON()).toMatchObject({ team_id: 'team-platform' });
  });

  test('the command menu groups skills and MCP prompts and says which cannot be used yet', async ({
    page,
  }) => {
    await page.goto('/agent/chat');
    const box = page.getByTestId('composer-input');
    await box.fill('/');
    const menu = page.locator('[data-agent-command-menu]');
    await expect(menu).toContainText('Skills');
    await expect(menu).toContainText('MCP');
    const linear = menu.getByRole('option', { name: /\/linear:triage/ });
    await expect(linear).toHaveAttribute('aria-disabled', 'true');
    await expect(linear).toContainText('Add your own credential for Linear first');
    await expect(menu.getByRole('option', { name: /\/github:review_pr/ })).toContainText('GitHub');
  });

  test('administrators say which internal hosts team and personal connections may reach', async ({
    page,
  }) => {
    await page.goto('/agent/settings/network');
    const section = page.locator('[data-agent-member-network]');
    await expect(section.getByLabel('Internal hosts')).toHaveValue('git.corp.example');
    await section.getByLabel('Internal ranges').fill('10.20.0.0/16\n172.16.4.0/24');
    const put = page.waitForRequest(isRequest('PUT', /\/api\/v1\/agent\/network-policy$/));
    await section.getByRole('button', { name: 'Save' }).click();
    expect((await put).postDataJSON()).toEqual({
      private_hosts: ['git.corp.example'],
      private_cidrs: ['10.20.0.0/16', '172.16.4.0/24'],
    });
  });
});
