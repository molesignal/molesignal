import '@/i18n';

import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import type { ScheduledReport } from '@/api/reports';
import { useAuthStore } from '@/stores/auth';

import {
  adaptReport,
  type DisplayReport,
  type ReportFormat,
} from '../reportTypes';
import { RunAsNote } from './RunAsNote';

const TAKEOVER = 'After you save, it runs with your access.';
const DANA = { id: 'u-dana', display_name: 'Dana Reyes', email: 'dana@example.com' };

/** A report as the backend lists it, last saved by Dana. */
function stored(overrides: Partial<ScheduledReport> = {}): ScheduledReport {
  return {
    id: 'r1',
    name: 'Weekly SLO',
    dashboard_id: 'd1',
    saved_view_id: null,
    cron: 'every:7d',
    recipients: [{ kind: 'email', target: 'sre@example.com' }],
    format: 'pdf',
    enabled: true,
    run_as_user_id: DANA.id,
    run_as_user: DANA,
    ...overrides,
  };
}

/** The same report as the list hands it to the workbench. */
function saved(overrides: Partial<ScheduledReport> = {}): DisplayReport {
  return adaptReport(stored(overrides));
}

/** A report that no signed-in user saved. */
function unowned(format: string): DisplayReport {
  return saved({ format, run_as_user_id: null, run_as_user: null });
}

function renderNote(
  report: DisplayReport | null,
  { format, canSave = true }: { format?: ReportFormat; canSave?: boolean } = {},
) {
  return render(
    <RunAsNote
      report={report}
      format={format ?? report?.format ?? 'pdf'}
      canSave={canSave}
    />,
  );
}

beforeEach(() => {
  useAuthStore.setState({
    token: 'token',
    ctx: {
      user_id: 'u-me',
      org_id: 'org-1',
      display_role: 'Editor',
      roles: [],
      display_name: 'Mei Lin',
      email: 'mei@example.com',
    },
  });
});

afterEach(() => {
  cleanup();
  useAuthStore.setState({ token: null, ctx: null });
});

describe('RunAsNote', () => {
  it('names the user a report runs as and says what saving does', () => {
    const { container } = renderNote(saved());

    expect(container.textContent).toBe(`Runs as Dana Reyes · ${TAKEOVER}`);
    // The email tells two people with the same name apart.
    expect(screen.getByText('Dana Reyes')).toHaveAttribute('title', 'dana@example.com');
  });

  it('uses the email of a user without a display name', () => {
    renderNote(
      saved({ run_as_user: { ...DANA, display_name: '' } }),
    );

    expect(screen.getByText('dana@example.com')).not.toHaveAttribute('title');
    expect(screen.getByText(TAKEOVER)).toBeInTheDocument();
  });

  it('shows the id of a user who no longer exists', () => {
    const { container } = renderNote(
      saved({ run_as_user_id: 'u-gone', run_as_user: null }),
    );

    expect(container.textContent).toBe(`Runs as u-gone · ${TAKEOVER}`);
  });

  it('shows the id when the backend does not name the user', () => {
    const report = stored({ run_as_user_id: 'u-dana' });
    delete report.run_as_user;

    const { container } = renderNote(adaptReport(report));

    expect(container.textContent).toBe(`Runs as u-dana · ${TAKEOVER}`);
  });

  it('names the reader without a change of hands when the report already runs as them', () => {
    const { container } = renderNote(
      saved({
        run_as_user_id: 'u-me',
        // The name the backend holds now, not the one the session started with.
        run_as_user: { id: 'u-me', display_name: 'Mei L.', email: 'mei@example.com' },
      }),
    );

    expect(container.textContent).toBe('Runs as Mei L. (you)');
    expect(screen.queryByText(TAKEOVER)).toBeNull();
  });

  it('runs a report that has not been saved yet as the reader', () => {
    const { container } = renderNote(null);

    expect(container.textContent).toBe('Runs as Mei Lin (you)');
  });

  it.each(['pdf', 'png'] as const)(
    'explains a %s report that no signed-in user saved',
    (format) => {
      renderNote(unowned(format));

      expect(screen.getByRole('status').textContent).toBe(
        `This report has no user to run as, so its ${format.toUpperCase()} deliveries fail. Save the report to run it with your access.`,
      );
      expect(screen.queryByText('Runs as')).toBeNull();
    },
  );

  it.each([
    ['a CSV report without a user', unowned('csv')],
    ['a CSV report with a user', saved({ format: 'csv' })],
    ['a JSON report without a user', unowned('json')],
    ['an SVG report without a user', unowned('svg')],
  ])('says nothing about %s', (_case, report) => {
    const { container } = renderNote(report);

    expect(container).toBeEmptyDOMElement();
  });

  it('follows the format chosen in the workbench', () => {
    // A PDF report about to be saved as CSV no longer reads data as anyone.
    const toCsv = renderNote(saved(), { format: 'csv' });
    expect(toCsv.container).toBeEmptyDOMElement();
    toCsv.unmount();

    // A CSV report never needed a user: choosing PDF makes the saver its user, nothing failed.
    const toPdf = renderNote(unowned('csv'), { format: 'pdf' });
    expect(toPdf.container.textContent).toBe('Runs as Mei Lin (you)');
    expect(screen.queryByRole('status')).toBeNull();
  });

  describe('for a reader who cannot save the report', () => {
    it('names the user the report runs as, and says nothing about saving', () => {
      const { container } = renderNote(saved(), { canSave: false });

      expect(container.textContent).toBe('Runs as Dana Reyes');
    });

    it('says who has to save a report that has no user to run as', () => {
      renderNote(unowned('pdf'), { canSave: false });

      expect(screen.getByRole('status').textContent).toBe(
        'This report has no user to run as, so its PDF deliveries fail. A signed-in user who can schedule reports has to save it.',
      );
    });

    it('says nothing about a report that does not exist yet', () => {
      const { container } = renderNote(null, { canSave: false });

      expect(container).toBeEmptyDOMElement();
    });
  });
});
