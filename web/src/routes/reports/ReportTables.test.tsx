import '@/i18n';

import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { TooltipProvider } from '@/shell/ui/tooltip';
import { useAuthStore } from '@/stores/auth';

import { ScheduleTable } from './ReportTables';
import { adaptReport, type DisplayReport } from './reportTypes';

const mocks = vi.hoisted(() => ({
  granted: new Set<string>(),
}));

vi.mock('@/product/actionAccess', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useActionAccess: ({ permission }: { permission: string }) =>
    mocks.granted.has(permission)
      ? { allowed: true, disabled: false }
      : { allowed: false, disabled: true, reason: `Requires ${permission}` },
}));

const REPORT: DisplayReport = adaptReport({
  id: 'r1',
  name: 'Weekly SLO',
  dashboard_id: 'd1',
  format: 'pdf',
  enabled: true,
  recipients: [{ kind: 'email', target: 'sre@example.com' }],
  run_as_user_id: 'u-dana',
  run_as_user: { id: 'u-dana', display_name: 'Dana Reyes', email: 'dana@example.com' },
});

function renderTable() {
  const onOpen = vi.fn();
  const onToggle = vi.fn();

  render(
    <TooltipProvider delayDuration={0}>
      <ScheduleTable
        reports={[REPORT]}
        dashboardNames={new Map([['d1', 'Web overview']])}
        savedViewNames={new Map()}
        latestDeliveryByReport={new Map()}
        exportingId={null}
        sharingId={null}
        toggling={false}
        onExport={vi.fn()}
        onOpen={onOpen}
        onShare={vi.fn()}
        onToggle={onToggle}
      />
    </TooltipProvider>,
  );

  return { row: screen.getByRole('row', { name: /Weekly SLO/ }), onOpen, onToggle };
}

beforeEach(() => {
  mocks.granted = new Set(['reports.schedule', 'reports.share']);
  useAuthStore.setState({
    token: 'token',
    ctx: { user_id: 'u-me', org_id: 'org-1', display_role: 'Editor', roles: [] },
  });
});

afterEach(() => {
  cleanup();
  useAuthStore.setState({ token: null, ctx: null });
});

describe('ScheduleTable', () => {
  it('opens a report to edit for a reader who can schedule reports', async () => {
    const user = userEvent.setup();
    const { row, onOpen } = renderTable();

    await user.click(within(row).getByRole('button', { name: 'Edit' }));
    await user.click(within(row).getByRole('button', { name: /Weekly SLO/ }));

    expect(onOpen).toHaveBeenCalledTimes(2);
    expect(onOpen).toHaveBeenLastCalledWith(REPORT);
    expect(within(row).queryByRole('button', { name: 'View' })).toBeNull();
  });

  it('opens a report to look at for a reader who cannot, and keeps the switch off limits', async () => {
    mocks.granted = new Set();
    const user = userEvent.setup();
    const { row, onOpen, onToggle } = renderTable();

    // What the report is, and who it runs as, is not hidden behind the right to change it.
    await user.click(within(row).getByRole('button', { name: 'View' }));
    await user.click(within(row).getByRole('button', { name: /Weekly SLO/ }));
    expect(onOpen).toHaveBeenCalledTimes(2);
    expect(within(row).queryByRole('button', { name: 'Edit' })).toBeNull();

    const status = within(row).getByRole('switch');
    expect(status).toBeDisabled();
    await user.click(status);
    expect(onToggle).not.toHaveBeenCalled();
  });
});
