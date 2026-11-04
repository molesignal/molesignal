import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as alertsApi from '@/api/alerts';
import { apmApi } from '@/api/apm';
import type { StatusPageComponent } from '@/api/statusPages';
import * as syntheticsApi from '@/api/synthetics';
import i18n from '@/i18n';
import type { AlertRule } from '@/types/alerting';

import { ComponentFormDrawer } from './ComponentFormDrawer';

vi.mock('@/api/alerts', () => ({ list: vi.fn() }));
vi.mock('@/api/synthetics', () => ({ listMonitors: vi.fn() }));
vi.mock('@/api/apm', () => ({ apmApi: { services: vi.fn() } }));
vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => ({ allowed: true, disabled: false, reason: undefined }),
}));

const listRules = vi.mocked(alertsApi.list);
const listMonitors = vi.mocked(syntheticsApi.listMonitors);
const apmServices = vi.mocked(apmApi.services);

const component: StatusPageComponent = {
  id: 'api',
  orgId: 'org',
  statusPageId: 'page',
  name: 'API',
  description: '',
  status: 'operational',
  visibility: 'enabled',
  lifecycle: 'active',
  position: 0,
  archivedAt: null,
  createdAt: 1,
  updatedAt: 1,
  dependencies: [
    { kind: 'technical_service', target: 'checkout-api', impact: 'outage' },
    { kind: 'alert_rule', target: 'rule-5xx', impact: 'degraded' },
  ],
  dependencyMode: 'follow',
  dependencyNotify: true,
  dependencyHealth: {
    status: 'partial_outage',
    causes: [{ kind: 'technical_service', target: 'checkout-api', status: 'partial_outage', incidentCount: 2 }],
    changedAt: 1,
    evaluatedAt: 2,
  },
};

function renderDrawer(onSubmit = vi.fn(), editing: StatusPageComponent | null = component) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ComponentFormDrawer
          open
          component={editing}
          groups={[]}
          busy={false}
          onOpenChange={vi.fn()}
          onSubmit={onSubmit}
        />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return onSubmit;
}

describe('ComponentFormDrawer', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-us');
    listRules.mockResolvedValue([{ id: 'rule-5xx', name: 'API 5xx rate' } as AlertRule]);
    listMonitors.mockResolvedValue([]);
    apmServices.mockResolvedValue({ items: [] } as unknown as Awaited<ReturnType<typeof apmApi.services>>);
  });

  afterEach(() => {
    vi.clearAllMocks();
    cleanup();
  });

  it('shows what a component depends on, how it is doing, and keeps both on save', async () => {
    const user = userEvent.setup();
    const onSubmit = renderDrawer();

    const health = document.querySelector('[data-dependency-health]') as HTMLElement;
    expect(within(health).getByText('Partial outage')).toBeTruthy();
    expect(within(health).getByText('2 open alert incidents', { exact: false })).toBeTruthy();
    expect(screen.getByDisplayValue('checkout-api')).toBeTruthy();
    // The rule is named once the list arrives, so the select is looked up each time; the
    // technical service before it is a text field with suggestions, a combobox as well.
    await waitFor(() =>
      expect(screen.getAllByRole('combobox', { name: 'Dependency' })[1]!.textContent).toContain('API 5xx rate'),
    );

    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({
      name: 'API',
      dependencies: component.dependencies,
      dependencyMode: 'follow',
      dependencyNotify: true,
    }));
  });

  it('sends the other way the health reaches the component', async () => {
    const user = userEvent.setup();
    const onSubmit = renderDrawer();

    await user.click(screen.getByText('Through automation rules'));
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({ dependencyMode: 'automation' }));
  });

  it('makes a group with nothing but a name and a description', async () => {
    const user = userEvent.setup();
    const onSubmit = renderDrawer(vi.fn(), null);

    await user.click(screen.getByText('Group'));
    await user.type(screen.getByRole('textbox', { name: /^Group name/ }), 'AI SRE');
    // A group has no status, visibility or dependencies of its own.
    expect(screen.queryByText('Dependencies')).toBeNull();
    expect(screen.queryByText('Current status')).toBeNull();
    await user.click(screen.getByRole('button', { name: 'Add group' }));

    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({
      name: 'AI SRE',
      kind: 'group',
      groupId: null,
      status: 'operational',
      visibility: 'enabled',
    }));
    expect(onSubmit.mock.calls[0]![0]).not.toHaveProperty('dependencies');
  });
});
