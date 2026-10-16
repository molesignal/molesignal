import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as alertsApi from '@/api/alerts';
import { apmApi } from '@/api/apm';
import * as businessServicesApi from '@/api/businessServices';
import type { BusinessService } from '@/api/businessServices';
import * as syntheticsApi from '@/api/synthetics';
import i18n from '@/i18n';

import { ServiceEditor } from './ServiceEditor';

vi.mock('@/api/alerts', () => ({ list: vi.fn() }));
vi.mock('@/api/apm', () => ({ apmApi: { services: vi.fn() } }));
vi.mock('@/api/businessServices', async (importOriginal) => ({
  ...(await importOriginal<typeof businessServicesApi>()),
  list: vi.fn(),
}));
vi.mock('@/api/synthetics', () => ({ listMonitors: vi.fn() }));
vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => ({ allowed: true, disabled: false, reason: undefined }),
}));

const payments: BusinessService = {
  id: 'payments',
  org_id: 'org',
  name: 'Payments',
  description: 'Card payments at checkout',
  dependencies: [
    { kind: 'synthetic_monitor', target: 'monitor-1', impact: 'outage' },
    { kind: 'technical_service', target: 'payment-api', impact: 'degraded' },
  ],
  health: { status: 'operational', causes: [], changed_at: null, evaluated_at: null, history: [] },
  created_by: null,
  created_at: 1,
  updated_at: 1,
  linked_components: [],
};

function renderEditor(onSubmit = vi.fn()) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <ServiceEditor open service={payments} busy={false} onOpenChange={vi.fn()} onSubmit={onSubmit} />
    </QueryClientProvider>,
  );
  return onSubmit;
}

describe('ServiceEditor', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
    vi.mocked(alertsApi.list).mockResolvedValue([]);
    vi.mocked(apmApi.services).mockRejectedValue(new Error('no apm'));
    vi.mocked(businessServicesApi.list).mockResolvedValue([payments]);
    vi.mocked(syntheticsApi.listMonitors).mockResolvedValue([
      { id: 'monitor-1', name: 'Checkout probe', lifecycle: 'active' },
    ] as Awaited<ReturnType<typeof syntheticsApi.listMonitors>>);
  });

  afterEach(() => cleanup());

  it('keeps the dependencies of a service being edited once the monitors arrive', async () => {
    const user = userEvent.setup();
    const onSubmit = renderEditor();

    await waitFor(() =>
      expect(screen.getAllByRole('combobox', { name: 'Dependency' })[0]?.textContent).toContain('Checkout probe'),
    );
    // A technical service is typed, with the names APM has seen offered as a list.
    expect(screen.getByDisplayValue('payment-api')).toHaveProperty('tagName', 'INPUT');

    await user.click(screen.getByRole('button', { name: 'Save' }));

    expect(onSubmit).toHaveBeenCalledWith({
      name: 'Payments',
      description: 'Card payments at checkout',
      dependencies: payments.dependencies,
    });
  });
});
