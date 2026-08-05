import '@/i18n';

import userEvent from '@testing-library/user-event';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter, Route, Routes } from 'react-router-dom';

import type { DashboardDraftPreview } from '@/api/intelligence/dashboardAuthoring';
import { createEmptyDashboardDefinition } from '@/dashboard-engine/model';

import { DashboardDraftPage } from './Page';

const api = vi.hoisted(() => ({
  draft: vi.fn(),
  propose: vi.fn(),
  execute: vi.fn(),
}));
const toast = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn() }));

vi.mock('@/api/intelligence/dashboardAuthoring', () => ({
  useDashboardDraft: api.draft,
  useProposeDashboardCreation: api.propose,
  useExecuteDashboardCreation: api.execute,
}));

vi.mock('@/dashboard-engine/DashboardRenderer', () => ({
  DashboardRenderer: ({
    dashboard,
    orgId,
    restricted,
  }: {
    dashboard: { title: string };
    orgId: string;
    restricted?: boolean;
  }) => (
    <div
      data-testid="dashboard-renderer"
      data-org={orgId}
      data-restricted={String(restricted)}
    >
      {dashboard.title}
    </div>
  ),
}));

vi.mock('@/shell/ui/sonner', () => ({ toast }));

vi.mock('@/stores/auth', () => {
  const state = { ctx: { org_id: 'org-1' } };
  return {
    useAuthStore: (selector: (value: typeof state) => unknown) => selector(state),
  };
});

const NOW = Date.now() * 1000;

function readyDraft(
  patch: Partial<DashboardDraftPreview> = {},
): DashboardDraftPreview {
  const model = createEmptyDashboardDefinition('Service health');
  return {
    draft_id: 'draft-1',
    model_hash: 'b'.repeat(64),
    status: 'ready',
    created_at: NOW - 60_000_000,
    expires_at: NOW + 600_000_000,
    compiled_model: model,
    warnings: [
      { code: 'EMPTY_RESULT', path: '/panels/0', message: 'No recent rows' },
    ],
    preflight: {
      panels: [
        {
          path: '/panels/0',
          title: 'Error rate',
          query_kind: 'promql',
          status: 'empty',
          tested_from_micros: NOW - 3_600_000_000,
          tested_to_micros: NOW,
          returned_rows: 0,
          scanned_rows: 0,
          took_ms: 4,
        },
      ],
      warnings: [],
      issues: [],
    },
    operation: null,
    dashboard_id: null,
    dashboard_route: null,
    can_propose: true,
    ...patch,
  };
}

function renderPage() {
  return render(
    <MemoryRouter initialEntries={['/ai/dashboard-drafts/draft-1']}>
      <Routes>
        <Route path="/ai/dashboard-drafts/:id" element={<DashboardDraftPage />} />
        <Route path="/dashboards/:id" element={<div>Created Dashboard route</div>} />
      </Routes>
    </MemoryRouter>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  api.draft.mockReturnValue({
    data: readyDraft(),
    isLoading: false,
    isError: false,
  });
  api.propose.mockReturnValue({ isPending: false, mutate: vi.fn() });
  api.execute.mockReturnValue({ isPending: false, mutate: vi.fn() });
});

afterEach(cleanup);

describe('Dashboard draft preview page', () => {
  it('renders the persisted model through the restricted production renderer', () => {
    renderPage();

    const renderer = screen.getByTestId('dashboard-renderer');
    expect(renderer.textContent).toBe('Service health');
    expect(renderer.dataset.org).toBe('org-1');
    expect(renderer.dataset.restricted).toBe('true');
    expect(screen.getByRole('complementary', { name: 'Creation review' })).toBeTruthy();
    expect(screen.getByText('No recent rows')).toBeTruthy();
    expect(screen.getByText('Empty result · 0 rows')).toBeTruthy();
  });

  it('submits only the draft id, hash, and human-readable proposal context', async () => {
    const user = userEvent.setup();
    const mutate = vi.fn();
    api.propose.mockReturnValue({ isPending: false, mutate });
    renderPage();

    await user.click(
      screen.getByRole('button', { name: 'Submit creation proposal' }),
    );

    expect(mutate).toHaveBeenCalledTimes(1);
    expect(mutate.mock.calls[0]?.[0]).toEqual({
      draftId: 'draft-1',
      expectedHash: 'b'.repeat(64),
      reason: 'The user reviewed the server-persisted Dashboard draft preview.',
      impact: 'Creates one native Dashboard without modifying existing Dashboards.',
    });
  });

  it('coalesces duplicate proposal clicks before mutation state rerenders', async () => {
    const user = userEvent.setup();
    const mutate = vi.fn();
    api.propose.mockReturnValue({ isPending: false, mutate });
    renderPage();

    await user.dblClick(
      screen.getByRole('button', { name: 'Submit creation proposal' }),
    );
    expect(mutate).toHaveBeenCalledTimes(1);
  });

  it('disables creation for an expired draft and while a mutation is pending', () => {
    api.draft.mockReturnValue({
      data: readyDraft({ status: 'expired', expires_at: NOW - 1, can_propose: false }),
      isLoading: false,
      isError: false,
    });
    renderPage();
    expect(
      (
        screen.getByRole('button', {
          name: 'Creation unavailable',
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true);
    cleanup();

    api.draft.mockReturnValue({
      data: readyDraft(),
      isLoading: false,
      isError: false,
    });
    api.propose.mockReturnValue({ isPending: true, mutate: vi.fn() });
    renderPage();
    expect(
      (screen.getByRole('button', { name: 'Submitting…' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });

  it('executes an approved confirmation with a stable key and follows the verified route', async () => {
    const user = userEvent.setup();
    api.draft.mockReturnValue({
      data: readyDraft({
        operation: {
          approval_id: 'approval-1',
          status: 'approved',
          required_approvals: 0,
          approved_reviews: 0,
        },
      }),
      isLoading: false,
      isError: false,
    });
    const mutate = vi.fn(
      (_input: unknown, callbacks: { onSuccess: (result: unknown) => void }) =>
        callbacks.onSuccess({
          status: 'succeeded',
          verification: { dashboard_route: '/dashboards/dashboard-1' },
        }),
    );
    api.execute.mockReturnValue({ isPending: false, mutate });
    renderPage();

    await user.click(screen.getByRole('button', { name: 'Confirm and create' }));

    expect(mutate.mock.calls[0]?.[0]).toEqual({
      approvalId: 'approval-1',
      idempotencyKey: `dashboard-draft-draft-1-${'b'.repeat(16)}`,
    });
    await waitFor(() => {
      expect(screen.getByText('Created Dashboard route')).toBeTruthy();
    });
    expect(toast.success).toHaveBeenCalledWith('Dashboard created.');
  });

  it('renders approval progress and structured retry guidance accessibly', () => {
    api.draft.mockReturnValue({
      data: readyDraft({
        operation: {
          approval_id: 'approval-2',
          status: 'pending',
          required_approvals: 2,
          approved_reviews: 1,
        },
        preflight: {
          panels: [],
          warnings: [],
          issues: [
            {
              code: 'UNKNOWN_STREAM',
              path: '/elements/0/queries/0',
              message: 'The selected stream no longer exists.',
            },
          ],
        },
      }),
      isLoading: false,
      isError: false,
    });
    renderPage();

    expect(
      screen.getByText(
        '1 of 2 reviews approved. Creation unlocks after the requirement is met.',
      ),
    ).toBeTruthy();
    expect(
      screen
        .getByRole('link', { name: 'View approval progress' })
        .getAttribute('href'),
    ).toBe('/intelligence/approvals');
    expect(screen.getAllByRole('alert')).toHaveLength(2);
    expect(
      screen.getByText('The selected stream no longer exists.'),
    ).toBeTruthy();
    expect(
      screen
        .getByRole('link', { name: 'Return to chat and regenerate' })
        .getAttribute('href'),
    ).toBe('/intelligence/chat');
  });

  it('renders tenant-safe lookup failures without mounting a preview', () => {
    api.draft.mockReturnValue({
      data: undefined,
      isLoading: false,
      isError: true,
      error: { response: { status: 404, data: { message: 'not found' } } },
    });
    renderPage();

    expect(screen.getByRole('alert')).toBeTruthy();
    expect(screen.queryByTestId('dashboard-renderer')).toBeNull();
  });
});
