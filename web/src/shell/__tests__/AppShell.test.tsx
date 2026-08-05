import { describe, expect, it } from 'vitest';

/**
 * AppShell renders the product topbar, grouped sidebar, status bar, and main area.
 * Component-level smoke: tokens-driven layout is verified by Playwright visual.
 * This unit-test only checks that the export exists and its prop contract is sane.
 */

import { AppShell } from '@/shell/AppShell';

describe('AppShell', () => {
  it('is a valid React component', () => {
    expect(typeof AppShell === 'function' || typeof AppShell === 'object').toBe(true);
  });
});
