import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { MemoryRouter, Route, Routes } from 'react-router-dom';

import { TooltipProvider } from '@/shell/ui/tooltip';

import { DashboardEditor } from './DashboardEditor';

afterEach(cleanup);

describe('DashboardEditor route', () => {
  it('opens directly on the live Dashboard edit canvas', async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });

    render(
      <MemoryRouter initialEntries={['/dashboards/new/edit']}>
        <QueryClientProvider client={queryClient}>
          <TooltipProvider delayDuration={0}>
            <Routes>
              <Route
                path="/dashboards/new/edit"
                element={<DashboardEditor />}
              />
            </Routes>
          </TooltipProvider>
        </QueryClientProvider>
      </MemoryRouter>,
    );

    expect(await screen.findByText('Editing dashboard')).toBeTruthy();
    expect(
      screen.getByText('This dashboard has no elements'),
    ).toBeTruthy();
    expect(
      screen.queryByRole('button', { name: 'Layout' }),
    ).toBeNull();
    expect(screen.queryByText('Back to layout')).toBeNull();
    expect(
      screen.getByRole('button', { name: /Time range:/ }),
    ).toBeTruthy();
  });

  it('offers only structured Dashboard settings pages', async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });

    render(
      <MemoryRouter initialEntries={['/dashboards/new/edit']}>
        <QueryClientProvider client={queryClient}>
          <TooltipProvider delayDuration={0}>
            <Routes>
              <Route
                path="/dashboards/new/edit"
                element={<DashboardEditor />}
              />
            </Routes>
          </TooltipProvider>
        </QueryClientProvider>
      </MemoryRouter>,
    );

    await screen.findByText('Editing dashboard');
    await user.click(screen.getByRole('button', { name: 'Settings' }));

    const dialog = screen.getByRole('dialog', { name: 'Dashboard settings' });
    expect(within(dialog).getByRole('tab', { name: 'General' })).toBeTruthy();
    expect(within(dialog).getByRole('tab', { name: 'Variables' })).toBeTruthy();
    expect(within(dialog).getByRole('tab', { name: 'Annotations' })).toBeTruthy();
    expect(within(dialog).getByRole('tab', { name: 'Links' })).toBeTruthy();
    expect(within(dialog).queryByRole('tab', { name: 'JSON model' })).toBeNull();
    expect(within(dialog).queryByText('Dashboard JSON')).toBeNull();

    const graphTooltip = within(dialog).getByRole('combobox', {
      name: 'Graph tooltip',
    });
    expect((graphTooltip as HTMLSelectElement).value).toBe('off');
    await user.selectOptions(graphTooltip, 'shared_crosshair');
    expect((graphTooltip as HTMLSelectElement).value).toBe('shared_crosshair');
  });

  it('keeps panel editing on the dedicated editor page', async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });

    render(
      <MemoryRouter initialEntries={['/dashboards/new/edit']}>
        <QueryClientProvider client={queryClient}>
          <TooltipProvider delayDuration={0}>
            <Routes>
              <Route
                path="/dashboards/new/edit"
                element={<DashboardEditor />}
              />
            </Routes>
          </TooltipProvider>
        </QueryClientProvider>
      </MemoryRouter>,
    );

    await screen.findByText('Editing dashboard');
    await user.click(screen.getByRole('button', { name: 'Add' }));
    await user.click(screen.getByRole('menuitem', { name: 'Panel' }));
    await user.click(screen.getByText('Back to dashboard'));

    expect(await screen.findByText('Editing dashboard')).toBeTruthy();
    expect(screen.queryByText('Grid position')).toBeNull();
    expect(screen.queryByText('Open panel editor')).toBeNull();

    const panel = screen.getByRole('region', { name: 'New panel' });
    await user.dblClick(panel);
    expect(screen.queryByText('Back to dashboard')).toBeNull();

    await user.click(
      screen.getByRole('button', { name: 'Open panel menu: New panel' }),
    );
    await user.click(screen.getByRole('menuitem', { name: 'Edit' }));
    expect(await screen.findByText('Back to dashboard')).toBeTruthy();
  });
});
