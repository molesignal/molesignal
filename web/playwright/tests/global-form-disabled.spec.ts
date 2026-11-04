import type { Locator, Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { setMockIntakeUsage } from '../fixtures/mockIntakeUsage';

/** A colour as the pixel it paints, so `rgb()` and `color(srgb …)` of one colour read alike. */
function paint(color: string): number[] {
  const context = document.createElement('canvas').getContext('2d');
  if (!context) return [];
  context.fillStyle = color;
  context.fillRect(0, 0, 1, 1);
  return Array.from(context.getImageData(0, 0, 1, 1).data);
}

async function fill(locator: Locator): Promise<number[]> {
  const color = await locator.evaluate((element) => getComputedStyle(element).backgroundColor);
  return locator.page().evaluate(paint, color);
}

async function token(page: Page, name: string): Promise<number[]> {
  const color = await page.evaluate((variable) => {
    const probe = document.createElement('span');
    probe.style.backgroundColor = `var(${variable})`;
    document.body.append(probe);
    const resolved = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return resolved;
  }, name);
  return page.evaluate(paint, color);
}

const READS = [
  'agent.use',
  'agent.usage.read',
  'agent.inbound_mcp.read',
  'agent.im.read',
  'agent.network_policy.read',
];

test.describe('global disabled treatment', () => {
  for (const { drawnBy, section, theme, controls } of [
    {
      drawnBy: 'the shell input and textarea',
      section: 'inbound-mcp',
      theme: 'light',
      controls: (page: Page) => [page.getByRole('spinbutton').first(), page.getByLabel('Allowed browser origins')],
    },
    {
      drawnBy: 'the shell input and textarea',
      section: 'inbound-mcp',
      theme: 'dark',
      controls: (page: Page) => [page.getByRole('spinbutton').first(), page.getByLabel('Allowed browser origins')],
    },
    {
      drawnBy: 'the form select',
      section: 'usage',
      theme: 'light',
      controls: (page: Page) => [page.getByRole('combobox', { name: 'Daily token limit' })],
    },
    {
      drawnBy: 'the fitted select',
      section: 'im',
      theme: 'light',
      controls: (page: Page) => [page.getByRole('combobox', { name: 'Where new group threads of Ops Feishu go' })],
    },
    {
      drawnBy: 'the form textarea',
      section: 'network',
      theme: 'light',
      controls: (page: Page) => [page.getByLabel('Internal hosts')],
    },
  ] as const) {
    test(`a field that cannot be used is gray: ${drawnBy} (${theme})`, async ({ page, mockServer }) => {
      await mountMockRoutes(page, mockServer.port, {
        role: 'viewer',
        capabilityPermissions: READS,
        theme,
      });
      await page.goto(`/agent/settings/${section}`);

      for (const control of controls(page)) {
        await expect(control).toBeDisabled();
        const gray = await token(page, '--bg-3');
        await expect.poll(() => fill(control)).toEqual(gray);
      }
    });
  }

  test('a field that cannot be used stays gray when the pointer rests on its label', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      role: 'viewer',
      capabilityPermissions: READS,
      theme: 'light',
    });
    await page.goto('/agent/settings/network');

    const hosts = page.getByLabel('Internal hosts');
    await expect(hosts).toBeDisabled();
    await hosts.locator('xpath=ancestor::label').getByText('Internal hosts', { exact: true }).hover();
    // A label passes its hover on to the field in it, disabled or not.
    await expect.poll(() => hosts.evaluate((element) => element.matches(':hover'))).toBe(true);
    // Longer than the transition, which would be on its way to white.
    await page.waitForTimeout(300);

    expect(await fill(hosts)).toEqual(await token(page, '--bg-3'));
  });

  test('a switch that cannot be used is gray, and still says whether it is on', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      role: 'viewer',
      capabilityPermissions: READS,
      theme: 'light',
    });
    await page.goto('/agent/settings/im');

    const on = page.getByRole('switch', { name: 'Answer in IM with Ops Feishu' });
    const off = page.getByRole('switch', { name: 'Answer in IM with Ops Slack' });
    await expect(on).toBeChecked();
    await expect(on).toBeDisabled();
    await expect(off).not.toBeChecked();
    await expect(off).toBeDisabled();

    await expect.poll(() => fill(on)).toEqual(await token(page, '--tx-disabled'));
    await expect.poll(() => fill(off)).toEqual(await token(page, '--bg-3'));
  });

  test('a switch that can be used keeps its colour', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, {
      role: 'viewer',
      capabilityPermissions: [...READS, 'agent.im.manage'],
      theme: 'light',
    });
    await page.goto('/agent/settings/im');

    const on = page.getByRole('switch', { name: 'Answer in IM with Ops Feishu' });
    await expect(on).toBeEnabled();
    await expect.poll(() => fill(on)).toEqual(await token(page, '--primary'));
  });

  test('a ticked checkbox and a chosen radio that cannot be changed are gray', async ({ page, mockServer }) => {
    await setMockIntakeUsage(mockServer.port, { seed_budgets: true });
    await mountMockRoutes(page, mockServer.port, { theme: 'light' });
    await page.goto('/streams/usage');
    await page
      .locator('tr', { has: page.getByText('ci-pipeline', { exact: true }) })
      .getByRole('button', { name: 'Set budget' })
      .click();

    const drawer = page.getByRole('dialog');
    const box = drawer.getByRole('checkbox', { name: /Dana Reyes/ });
    const radio = drawer.getByRole('radio', { name: /Each month/ });
    await box.click();
    await radio.click();
    await expect(box).toBeChecked();
    await expect(radio).toBeChecked();
    // This drawer disables neither: it is how they look that is under test.
    for (const control of [box, radio]) {
      await control.evaluate((element) => {
        (element as HTMLButtonElement).disabled = true;
      });
      await expect(control).toBeDisabled();
    }

    await expect.poll(() => fill(box)).toEqual(await token(page, '--tx-disabled'));
    await expect.poll(() => fill(radio)).toEqual(await token(page, '--bg-3'));
    const inkOf = async () => {
      const color = await radio.evaluate((element) => getComputedStyle(element).color);
      return page.evaluate(paint, color);
    };
    await expect.poll(inkOf).toEqual(await token(page, '--tx-disabled'));
  });
});
