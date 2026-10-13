import '@/i18n';

import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { Dialog, DialogContent, DialogTitle } from '@/shell/ui/dialog';
import { useAuthStore } from '@/stores/auth';

import { adaptReport, type DisplayReport } from '../reportTypes';
import { WorkbenchFooter } from './Footer';
import { workbenchMode } from './mode';

const mocks = vi.hoisted(() => ({
  /** The permissions the reader holds. */
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
  run_as_user_id: 'u-dana',
  run_as_user: { id: 'u-dana', display_name: 'Dana Reyes', email: 'dana@example.com' },
});

function renderFooter(report: DisplayReport | null) {
  const onDelete = vi.fn();
  const onClose = vi.fn();
  const onSubmit = vi.fn((event: React.FormEvent) => event.preventDefault());

  render(
    <Dialog open>
      <DialogContent aria-describedby={undefined}>
        <DialogTitle>Report</DialogTitle>
        <form id="report-form" onSubmit={onSubmit} />
        <WorkbenchFooter
          mode={workbenchMode(report, mocks.granted.has('reports.schedule'))}
          report={report}
          format="pdf"
          formId="report-form"
          invalid={false}
          saving={false}
          deleting={false}
          onDelete={onDelete}
          onClose={onClose}
        />
      </DialogContent>
    </Dialog>,
  );

  return { onDelete, onClose, onSubmit };
}

beforeEach(() => {
  mocks.granted = new Set(['reports.schedule', 'reports.delete']);
  useAuthStore.setState({
    token: 'token',
    ctx: { user_id: 'u-me', org_id: 'org-1', display_role: 'Editor', roles: [] },
  });
});

afterEach(() => {
  cleanup();
  useAuthStore.setState({ token: null, ctx: null });
});

describe('workbenchMode', () => {
  it('tells a new report, one the reader can save and one the reader can only look at apart', () => {
    expect(workbenchMode(null, true)).toBe('new');
    expect(workbenchMode(null, false)).toBe('new');
    expect(workbenchMode(REPORT, true)).toBe('edit');
    expect(workbenchMode(REPORT, false)).toBe('view');
  });
});

describe('WorkbenchFooter', () => {
  it('states who the report runs as beside the control that saves it', async () => {
    const user = userEvent.setup();
    const { onDelete, onClose, onSubmit } = renderFooter(REPORT);

    const save = screen.getByRole('button', { name: 'Save changes' });
    const note = screen.getByText('After you save, it runs with your access.');
    // The statement comes right before the controls it is about.
    expect(
      note.compareDocumentPosition(save) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
    expect(screen.getByText('Dana Reyes')).toBeInTheDocument();

    await user.click(save);
    expect(onSubmit).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onClose).toHaveBeenCalledTimes(1);
    await user.click(screen.getByRole('button', { name: 'Delete schedule' }));
    expect(onDelete).toHaveBeenCalledTimes(1);
  });

  it('saves a new report under its own label, with nothing to delete', () => {
    renderFooter(null);

    expect(screen.getByRole('button', { name: 'Save and enable' })).toBeEnabled();
    expect(screen.queryByRole('button', { name: 'Delete schedule' })).toBeNull();
  });

  it('gives a reader who can only look a way out, and no control that saves or deletes', async () => {
    mocks.granted = new Set();
    const user = userEvent.setup();
    const { onClose } = renderFooter(REPORT);

    expect(screen.getByText('Dana Reyes')).toBeInTheDocument();
    expect(screen.queryByText('After you save, it runs with your access.')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Save changes' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Cancel' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Delete schedule' })).toBeNull();

    // The dialog's own corner control is also named Close; the footer's is the one with text.
    await user.click(screen.getByText('Close', { selector: 'button' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('keeps delete for a reader who may delete but not save', () => {
    mocks.granted = new Set(['reports.delete']);
    renderFooter(REPORT);

    expect(screen.getByRole('button', { name: 'Delete schedule' })).toBeEnabled();
    expect(screen.queryByRole('button', { name: 'Save changes' })).toBeNull();
  });

  it('explains why a new report cannot be saved', () => {
    mocks.granted = new Set();
    renderFooter(null);

    expect(screen.getByRole('button', { name: 'Save and enable' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeEnabled();
    // Nothing is saved, so there is nobody the report runs as.
    expect(screen.queryByText('Runs as')).toBeNull();
  });
});
