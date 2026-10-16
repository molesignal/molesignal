import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as businessServicesApi from '@/api/businessServices';
import type { BusinessService } from '@/api/businessServices';
import type { StatusPageComponent } from '@/api/statusPages';
import i18n from '@/i18n';

import { ComponentFormDrawer } from './ComponentFormDrawer';

vi.mock('@/api/businessServices', () => ({
  list: vi.fn(),
}));
vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => ({ allowed: true, disabled: false, reason: undefined }),
}));

const listServices = vi.mocked(businessServicesApi.list);

const checkout: BusinessService = {
  id: 'checkout',
  org_id: 'org',
  name: 'Checkout',
  description: '',
  dependencies: [],
  health: { status: 'operational', causes: [], changed_at: 1, evaluated_at: 1, history: [] },
  created_by: null,
  created_at: 1,
  updated_at: 1,
  linked_components: [],
};

const component: StatusPageComponent = {
  id: 'api',
  org_id: 'org',
  status_page_id: 'page',
  name: 'API',
  description: '',
  status: 'operational',
  visibility: 'enabled',
  lifecycle: 'active',
  position: 0,
  archived_at: null,
  created_at: 1,
  updated_at: 1,
  business_service: { business_service_id: 'checkout', mode: 'follow', notify_subscribers: true },
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
    listServices.mockResolvedValue([checkout]);
  });

  afterEach(() => cleanup());

  it('keeps the business service of a component being edited once the list arrives', async () => {
    const user = userEvent.setup();
    const onSubmit = renderDrawer();

    // The select is drawn again once the list is no longer loading, so it is looked up each time.
    await waitFor(() =>
      expect(screen.getByRole('combobox', { name: 'Business service' }).textContent).toContain('Checkout'),
    );

    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({ input: expect.objectContaining({ name: 'API' }), link: undefined }),
    );
  });

  it('makes a group with nothing but a name and a description', async () => {
    const user = userEvent.setup();
    const onSubmit = renderDrawer(vi.fn(), null);

    await user.click(screen.getByText('Group'));
    await user.type(screen.getByRole('textbox', { name: /^Group name/ }), 'AI SRE');
    // A group has no status, visibility or business service of its own.
    expect(screen.queryByRole('combobox', { name: 'Business service' })).toBeNull();
    expect(screen.queryByText('Current status')).toBeNull();
    await user.click(screen.getByRole('button', { name: 'Add group' }));

    expect(onSubmit).toHaveBeenCalledWith({
      input: expect.objectContaining({
        name: 'AI SRE',
        kind: 'group',
        group_id: null,
        status: 'operational',
        visibility: 'enabled',
      }),
      link: undefined,
    });
  });

  it('sends the link again when how the component follows the service changes', async () => {
    const user = userEvent.setup();
    const onSubmit = renderDrawer();

    await waitFor(() =>
      expect(screen.getByRole('combobox', { name: 'Business service' }).textContent).toContain('Checkout'),
    );
    await user.click(screen.getByText('Through automation rules'));
    await user.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({
        link: { business_service_id: 'checkout', mode: 'automation', notify_subscribers: true },
      }),
    );
  });
});
