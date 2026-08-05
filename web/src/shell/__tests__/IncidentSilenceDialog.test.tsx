import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import * as mutesApi from '@/api/mutes';
import { IncidentSilenceDialog } from '@/shell/incident/SilenceDialog';

vi.mock('@/api/mutes', () => ({
  silenceIncident: vi.fn(),
}));

vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => ({
    allowed: true,
    disabled: false,
    reason: undefined,
  }),
}));

vi.mock('@/shell/ui/sonner', () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
  },
}));

const silenceIncident = vi.mocked(mutesApi.silenceIncident);

describe('IncidentSilenceDialog', () => {
  beforeEach(() => {
    silenceIncident.mockReset();
  });

  it('creates a silence scoped to the selected incident', async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    const queryClient = new QueryClient({
      defaultOptions: {
        mutations: { retry: false },
        queries: { retry: false },
      },
    });
    silenceIncident.mockResolvedValue({
      id: 'mute-1',
      org_id: 'org-1',
      name: 'incident-incident-1',
      enabled: true,
      matchers: [],
      window: { type: 'fixed', start: 1, end: 2 },
      comment: '',
      created_by: null,
      created_at: 1,
      updated_at: 1,
    });

    render(
      <QueryClientProvider client={queryClient}>
        <IncidentSilenceDialog
          incidentId="incident-1"
          incidentName="Checkout error rate"
          open
          onOpenChange={onOpenChange}
        />
      </QueryClientProvider>,
    );

    expect(
      screen.getByText(/only this firing incident/i),
    ).not.toBeNull();

    await user.click(
      screen.getByRole('button', { name: 'Silence incident' }),
    );

    await waitFor(() => {
      expect(silenceIncident).toHaveBeenCalledWith('incident-1', {
        duration_secs: 3_600,
        comment: '',
      });
      expect(onOpenChange).toHaveBeenCalledWith(false);
    });
  });
});
