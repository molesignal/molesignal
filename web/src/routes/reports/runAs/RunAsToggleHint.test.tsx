import '@/i18n';

import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { ScheduledReport } from '@/api/reports';
import { Switch } from '@/shell/ui/switch';
import { TooltipProvider } from '@/shell/ui/tooltip';
import { useAuthStore } from '@/stores/auth';

import { adaptReport } from '../reportTypes';
import { RunAsToggleHint } from './RunAsToggleHint';

const HINT =
  'Turning this on or off saves the report, which then runs with your access.';

function renderSwitch(
  overrides: Partial<ScheduledReport>,
  { disabled = false, onToggle = vi.fn() } = {},
) {
  const report = adaptReport({
    id: 'r1',
    name: 'Weekly SLO',
    dashboard_id: 'd1',
    format: 'pdf',
    enabled: true,
    run_as_user_id: 'u-dana',
    ...overrides,
  });

  render(
    <TooltipProvider delayDuration={0}>
      <RunAsToggleHint report={report} disabled={disabled}>
        {(describedBy) => (
          <Switch
            checked={report.enabled}
            disabled={disabled}
            onCheckedChange={onToggle}
            aria-label="Pause Weekly SLO"
            aria-describedby={describedBy}
          />
        )}
      </RunAsToggleHint>
    </TooltipProvider>,
  );

  return { toggle: screen.getByRole('switch'), onToggle };
}

beforeEach(() => {
  useAuthStore.setState({
    token: 'token',
    ctx: { user_id: 'u-me', org_id: 'org-1', display_role: 'Editor', roles: [] },
  });
});

afterEach(() => {
  cleanup();
  useAuthStore.setState({ token: null, ctx: null });
});

describe('RunAsToggleHint', () => {
  it("says that the switch takes over someone else's report, and leaves the switch working", async () => {
    const user = userEvent.setup();
    const { toggle, onToggle } = renderSwitch({ run_as_user_id: 'u-dana' });

    // Said to someone who reaches the switch without a pointer, too.
    expect(toggle).toHaveAccessibleDescription(HINT);

    await user.hover(toggle);
    expect((await screen.findByRole('tooltip')).textContent).toBe(HINT);

    // The switch still reads as on, and still switches.
    expect(toggle).toHaveAttribute('data-state', 'checked');
    await user.click(toggle);
    expect(onToggle).toHaveBeenCalledWith(false);
  });

  it('says so for a PDF report that no signed-in user saved', async () => {
    const user = userEvent.setup();
    const { toggle } = renderSwitch({ run_as_user_id: null });

    await user.hover(toggle);
    expect((await screen.findByRole('tooltip')).textContent).toBe(HINT);
  });

  it.each([
    ['the report already runs as the reader', { run_as_user_id: 'u-me' }, false],
    ['the format renders no page', { format: 'csv', run_as_user_id: 'u-dana' }, false],
    ['an SVG report has no user', { format: 'svg', run_as_user_id: null }, false],
    ['an SVG report has another user', { format: 'svg', run_as_user_id: 'u-dana' }, false],
    ['the switch cannot be used', { run_as_user_id: 'u-dana' }, true],
  ])('says nothing when %s', async (_case, overrides, disabled) => {
    const user = userEvent.setup();
    const { toggle } = renderSwitch(overrides, { disabled });

    expect(toggle).toHaveAccessibleDescription('');

    await user.hover(toggle);
    // Long enough for a tooltip to open if there were one.
    await expect(
      screen.findByRole('tooltip', {}, { timeout: 200 }),
    ).rejects.toThrow();
  });
});
