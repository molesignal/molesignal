import type { Locator } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

interface FocusDecoration {
  outlineStyle: string;
  ringShadow: string;
  borderColor: string;
  accentColor: string;
}

async function focusDecoration(
  locator: Locator,
): Promise<FocusDecoration> {
  await locator.focus();
  return locator.evaluate((element) => {
    const probe = document.createElement('span');
    probe.style.color = 'var(--indigo)';
    document.body.append(probe);
    const style = getComputedStyle(element);
    const probeStyle = getComputedStyle(probe);
    const result = {
      outlineStyle: style.outlineStyle,
      ringShadow: style.getPropertyValue('--tw-ring-shadow').trim(),
      borderColor: style.borderColor,
      accentColor: probeStyle.color,
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

test.describe('global focus treatment', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('uses one neutral border for text, native select, and combobox controls', async ({
    page,
  }) => {
    await page.goto('/rum/sessions');

    expectQuietFocus(
      await focusDecoration(
        page.getByPlaceholder(
          'Search users, pages, browsers, versions, or sessions…',
        ),
      ),
    );
    expectQuietFocus(
      await focusDecoration(
        page
          .locator('label')
          .filter({ hasText: 'Browser' })
          .locator('select'),
      ),
    );

    await page.goto('/intelligence/chat');
    expectQuietFocus(
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
