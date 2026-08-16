import type { Locator } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

interface FocusDecoration {
  outlineStyle: string;
  ringShadow: string;
  borderColor: string;
  accentColor: string;
  backgroundColor: string;
  controlBackgroundColor: string;
  backgroundChannels: number[];
  controlBackgroundChannels: number[];
  filter: string;
}

async function focusDecoration(
  locator: Locator,
): Promise<FocusDecoration> {
  await locator.focus();
  return locator.evaluate((element) => {
    const probe = document.createElement('span');
    probe.style.color = 'var(--indigo)';
    probe.style.backgroundColor = 'var(--bg-2)';
    document.body.append(probe);
    const style = getComputedStyle(element);
    const probeStyle = getComputedStyle(probe);
    const canvas = document.createElement('canvas');
    canvas.width = 1;
    canvas.height = 1;
    const context = canvas.getContext('2d');
    const colorChannels = (color: string): number[] => {
      if (!context) return [];
      context.clearRect(0, 0, 1, 1);
      context.fillStyle = color;
      context.fillRect(0, 0, 1, 1);
      return Array.from(context.getImageData(0, 0, 1, 1).data);
    };
    const result = {
      outlineStyle: style.outlineStyle,
      ringShadow: style.getPropertyValue('--tw-ring-shadow').trim(),
      borderColor: style.borderColor,
      accentColor: probeStyle.color,
      backgroundColor: style.backgroundColor,
      controlBackgroundColor: probeStyle.backgroundColor,
      backgroundChannels: colorChannels(style.backgroundColor),
      controlBackgroundChannels: colorChannels(probeStyle.backgroundColor),
      filter: style.filter,
    };
    probe.remove();
    return result;
  });
}

function expectQuietFocus(decoration: FocusDecoration): void {
  expect(decoration.outlineStyle).toBe('none');
  expect(decoration.ringShadow).toContain('#0000');
  expect(decoration.borderColor).not.toBe(decoration.accentColor);
}

function expectUnfilteredFormFocus(decoration: FocusDecoration): void {
  expectQuietFocus(decoration);
  expect(decoration.filter).toBe('none');
}

test.describe('global focus treatment', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('keeps text and combobox fills stable without a focus filter', async ({
    page,
  }) => {
    await page.goto('/rum/sessions');

    const searchDecoration = await focusDecoration(
      page.getByPlaceholder(
        'Search users, pages, browsers, versions, or sessions…',
      ),
    );
    expectUnfilteredFormFocus(searchDecoration);
    expect(searchDecoration.backgroundChannels).toEqual(
      searchDecoration.controlBackgroundChannels,
    );

    expectUnfilteredFormFocus(
      await focusDecoration(
        page.getByRole('combobox', { name: 'Browser' }),
      ),
    );

    await page.goto('/agent/chat');
    expectUnfilteredFormFocus(
      await focusDecoration(
        page.getByRole('combobox', { name: 'Time' }),
      ),
    );
  });

  test('does not draw a focus frame around dashboard toolbar buttons', async ({
    page,
  }) => {
    await page.goto('/dashboards/d1');

    expectQuietFocus(
      await focusDecoration(
        page.getByRole('button', {
          name: 'Refresh all panels',
        }),
      ),
    );
    expectQuietFocus(
      await focusDecoration(
        page.getByRole('button', {
          name: 'Refresh mode and interval',
        }),
      ),
    );
  });
});
