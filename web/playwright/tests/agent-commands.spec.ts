import { readFileSync } from 'node:fs';

import type { Locator, Page, Request } from '@playwright/test';

import { MOCK_AGENT_COMMANDS } from '../fixtures/mockAgentCommands';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const ANSWER = 'The selected data scope is available.';

const composer = (page: Page) => page.getByTestId('composer-input');
const menu = (page: Page) => page.locator('[data-agent-command-menu]');
const notice = (page: Page) => page.locator('[data-agent-command-notice]');

const isPost = (request: Request) =>
  request.method() === 'POST' && /\/api\/v1\/agent\/chat\/[^/]+\/messages$/.test(request.url());

/** Opens the menu once, so the composer knows the commands before anything is typed. */
async function commandsReady(page: Page) {
  await composer(page).fill('/');
  await expect(menu(page).getByRole('option')).toHaveCount(MOCK_AGENT_COMMANDS.length);
  await clearComposer(composer(page));
}
/** Empties the composer the way a person does: select all, then delete. */
async function clearComposer(box: Locator) {
  await box.click();
  await box.press('ControlOrMeta+a');
  await box.press('Backspace');
}


test.describe('Mole Agent slash commands', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('a slash lists the commands; letters narrow them and the keys pick one and its word', async ({
    page,
  }) => {
    await page.goto('/agent/chat');
    const box = composer(page);
    await box.fill('/');
    await expect(menu(page)).toHaveAttribute('data-agent-command-menu', 'commands');
    await expect(box).toHaveAttribute('aria-expanded', 'true');
    await expect(menu(page).getByRole('option').first()).toContainText('List the commands');

    // Names come before descriptions that mention the letters.
    await box.pressSequentially('mod');
    const options = menu(page).getByRole('option');
    await expect(options).toHaveCount(2);
    await expect(options.nth(0)).toContainText('/mode');
    await expect(options.nth(1)).toContainText('/model');
    await expect(options.nth(0)).toHaveAttribute('aria-selected', 'true');
    await box.press('ArrowDown');
    await expect(options.nth(1)).toHaveAttribute('aria-selected', 'true');
    await expect(box).toHaveAttribute('aria-activedescendant', /option-1$/);
    await box.press('ArrowUp');

    // Tab fills the name in; the words of /mode follow.
    await box.press('Tab');
    await expect(box).toHaveText('/mode ');
    await expect(menu(page)).toHaveAttribute('data-agent-command-menu', 'arguments');
    await expect(menu(page).getByRole('option')).toHaveCount(4);
    // A word is found by what it means too; the words that start with the letters come first.
    await box.pressSequentially('de');
    await expect(menu(page).getByRole('option').first()).toContainText('deep');

    // Enter on a word puts it in the composer; Enter again runs the command here: nothing
    // is sent.
    let posted = false;
    page.on('request', (request) => {
      if (isPost(request)) posted = true;
    });
    await box.press('Enter');
    await expect(box).toHaveText('/mode deep');
    await expect(menu(page)).toHaveCount(0);
    await box.press('Enter');
    await expect(box).toHaveText('');
    await expect(page.getByText('Now using Deep investigation')).toBeVisible();
    expect(posted).toBe(false);

    // The next question goes out in that mode, with the model picked by its name.
    // A word picked from the list goes in by its value; Enter again runs it.
    await box.fill('/model internal qwen');
    await box.press('Enter');
    await expect(box).toHaveText('/model Internal Qwen');
    await box.press('Enter');
    await expect(page.getByText('Now using Internal Qwen')).toBeVisible();

    const outbound = page.waitForRequest(isPost);
    await box.fill('Why is checkout slow?');
    await box.press('Enter');
    const body = (await outbound).postDataJSON() as { analysis_mode?: string; provider_id?: string };
    expect(body.analysis_mode).toBe('root_cause');
    expect(body.provider_id).toBe('compatible-qwen');
  });

  test('a task command goes out as typed and reads as a capsule in the chat', async ({ page }) => {
    await page.goto('/agent/chat');
    await commandsReady(page);

    const box = composer(page);
    await box.fill('/query ');
    await expect(menu(page)).toHaveAttribute('data-agent-command-menu', 'hint');
    await expect(menu(page)).toContainText('<question>');

    const outbound = page.waitForRequest(isPost);
    await box.pressSequentially('errors by service');
    await box.press('Enter');
    expect(((await outbound).postDataJSON() as { content: string }).content).toBe(
      '/query errors by service',
    );

    await expect(page.getByText(ANSWER).first()).toBeVisible();
    const question = page.locator('[data-message-role="user"]').last();
    await expect(question.locator('[data-agent-command="query"]')).toContainText('/query');
    await expect(question).toContainText('errors by service');
  });

  test('an unknown command is not sent, and a backslash sends the slash as text', async ({
    page,
  }) => {
    await page.goto('/agent/chat');
    await commandsReady(page);
    let posted = false;
    page.on('request', (request) => {
      if (isPost(request)) posted = true;
    });

    const box = composer(page);
    await box.fill('/frobnicate now');
    await box.press('Enter');
    await expect(notice(page)).toHaveAttribute('data-agent-command-notice', 'unknown_command');
    await expect(notice(page)).toContainText('There is no /frobnicate command.');
    await expect(box).toHaveText('/frobnicate now');
    expect(posted).toBe(false);

    await box.fill('/mode fast');
    await box.press('Enter');
    await expect(notice(page)).toHaveAttribute('data-agent-command-notice', 'argument_invalid');
    await expect(notice(page)).toContainText('auto | quick | deep | query');

    await box.fill('\\/etc/hosts changed?');
    await expect(notice(page)).toHaveCount(0);
    await box.press('Enter');
    const question = page.locator('[data-message-role="user"]').last();
    await expect(question).toContainText('/etc/hosts changed?');
    await expect(question).not.toContainText('\\');
    await expect(question.locator('[data-agent-command]')).toHaveCount(0);
    await expect(page.getByText(ANSWER)).toBeVisible();
  });

  test('/rca offers the open incidents, and one the backend cannot find comes back', async ({
    page,
  }) => {
    await page.goto('/agent/chat');
    await commandsReady(page);

    const box = composer(page);
    await box.fill('/rca ');
    const options = menu(page).getByRole('option');
    await expect(options).toHaveCount(3);
    await expect(options.first()).toContainText('inc-2291');
    await expect(options.first()).toContainText('Public API answers 16% of requests with 502');

    await box.fill('/rca inc-0000');
    await box.press('Enter');
    await expect(notice(page)).toHaveAttribute(
      'data-agent-command-notice',
      'target_not_found',
    );
    await expect(box).toHaveText('/rca inc-0000');

    // A click puts the incident in the composer; Enter sends it.
    await box.fill('/rca ');
    await options.first().click();
    await expect(box).toHaveText('/rca inc-2291');
    const outbound = page.waitForRequest(isPost);
    await box.press('Enter');
    expect(((await outbound).postDataJSON() as { content: string }).content).toBe('/rca inc-2291');
    await expect(
      page.locator('[data-message-role="user"]').last().locator('[data-agent-command="rca"]'),
    ).toBeVisible();
  });

  test('/help opens every command and /export downloads the chat', async ({ page }) => {
    await page.goto('/agent/chat');
    await commandsReady(page);

    // Enter on the command in the menu chooses it; Enter again runs it.
    const box = composer(page);
    await box.fill('/export');
    await box.press('Enter');
    await expect(box).toHaveText('/export ');
    await box.press('Enter');
    await expect(notice(page)).toHaveAttribute('data-agent-command-notice', 'nothing_to_export');

    await box.fill('Why is checkout slow?');
    await box.press('Enter');
    await expect(page.getByText(ANSWER)).toBeVisible();

    const download = page.waitForEvent('download');
    await box.fill('/export ');
    await box.press('Enter');
    const file = await download;
    expect(file.suggestedFilename()).toMatch(/\.md$/);
    const markdown = readFileSync(await file.path(), 'utf8');
    expect(markdown).toContain('## You ·');
    expect(markdown).toContain('Why is checkout slow?');
    expect(markdown).toContain(ANSWER);

    await box.fill('/help ');
    await box.press('Enter');
    await expect(box).toHaveText('/');
    await expect(menu(page).getByRole('option')).toHaveCount(MOCK_AGENT_COMMANDS.length);
  });

  test('in the panel, Escape closes the menu before the panel', async ({ page }) => {
    await page.goto('/logs');
    await page.getByTestId('mole-agent-trigger').click();
    const panel = page.locator('aside[aria-label="Mole Agent"]');
    await expect(panel).not.toHaveAttribute('inert', '');

    const box = panel.getByTestId('composer-input');
    await box.fill('/');
    await expect(panel.locator('[data-agent-command-menu]')).toBeVisible();
    await box.press('Escape');
    await expect(panel.locator('[data-agent-command-menu]')).toHaveCount(0);
    await expect(panel).not.toHaveAttribute('inert', '');

    // Typing again opens it again; Escape with no menu closes the panel.
    await box.pressSequentially('h');
    await expect(panel.locator('[data-agent-command-menu]')).toBeVisible();
    await box.press('Escape');
    await box.press('Escape');
    await expect(panel).toHaveAttribute('inert', '');
  });
});
