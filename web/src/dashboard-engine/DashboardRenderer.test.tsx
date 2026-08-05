import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import userEvent from '@testing-library/user-event';
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter } from 'react-router-dom';

import { DEFAULT_WINDOW, useTimeStore } from '@/stores/useTimeStore';

import { createDashboardPanel } from './factories';
import { createEmptyDashboardDefinition } from './model';
import type {
  DashboardElement,
  DataFrame,
  VisualizationType,
} from './schema';
import {
  DashboardRenderer,
  type DashboardPanelQueryExecutor,
} from './DashboardRenderer';

afterEach(() => {
  cleanup();
  useTimeStore.setState({ window: DEFAULT_WINDOW });
});

describe('DashboardRenderer chart integration', () => {
  it('hides privileged panel actions when product capabilities are unavailable', async () => {
    const user = userEvent.setup();
    const dashboard = createEmptyDashboardDefinition('Restricted actions');
    dashboard.refreshSettings = {
      enabled: false,
      mode: 'off',
      allowedIntervals: ['off'],
    };
    const [panel] = chartPanels(['stat']);
    dashboard.elements = panel ? [panel] : [];
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });

    render(
      <MemoryRouter>
        <QueryClientProvider client={queryClient}>
          <DashboardRenderer
            dashboard={dashboard}
            orgId="org-1"
            panelQueryExecutor={async () => [chartFrame()]}
          />
        </QueryClientProvider>
      </MemoryRouter>,
    );

    const menu = await screen.findByRole('button', {
      name: 'Open panel menu: stat',
    });
    await user.click(menu);
    await screen.findByText('Inspect queries');

    expect(screen.queryByText('Mole Agent analysis')).toBeNull();
    expect(screen.queryByText('Create alert')).toBeNull();
  });

  it('reveals the complete panel title from the truncated heading', async () => {
    const dashboard = createEmptyDashboardDefinition('Panel title tooltip');
    dashboard.refreshSettings = {
      enabled: false,
      mode: 'off',
      allowedIntervals: ['off'],
    };
    const [panel] = chartPanels(['stat']);
    if (!panel) throw new Error('Expected a stat panel');
    panel.title = 'A complete panel title that does not fit in a narrow card';
    dashboard.elements = [panel];
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });

    render(
      <MemoryRouter>
        <QueryClientProvider client={queryClient}>
          <DashboardRenderer
            dashboard={dashboard}
            orgId="org-1"
            panelQueryExecutor={async () => [chartFrame()]}
          />
        </QueryClientProvider>
      </MemoryRouter>,
    );

    const heading = await screen.findByRole('heading', {
      name: panel.title,
    });
    fireEvent.focus(heading);

    expect((await screen.findByRole('tooltip')).textContent).toBe(panel.title);
  });

  it('shows the panel description from its info button on hover or click', async () => {
    const user = userEvent.setup();
    const dashboard = createEmptyDashboardDefinition('Panel description tooltip');
    dashboard.refreshSettings = {
      enabled: false,
      mode: 'off',
      allowedIntervals: ['off'],
    };
    const [panel] = chartPanels(['stat']);
    if (!panel) throw new Error('Expected a stat panel');
    const description = 'Five-minute request rate for the selected service.';
    panel.description = description;
    dashboard.elements = [panel];
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });

    render(
      <MemoryRouter>
        <QueryClientProvider client={queryClient}>
          <DashboardRenderer
            dashboard={dashboard}
            orgId="org-1"
            panelQueryExecutor={async () => [chartFrame()]}
          />
        </QueryClientProvider>
      </MemoryRouter>,
    );

    const descriptionButton = await screen.findByRole('button', {
      name: 'Description: stat',
    });
    expect(screen.queryByText(description)).toBeNull();

    await user.hover(descriptionButton);
    expect((await screen.findByRole('tooltip')).textContent).toBe(description);

    await user.unhover(descriptionButton);
    fireEvent.pointerMove(document.body, {
      pointerType: 'mouse',
      clientX: 500,
      clientY: 500,
    });
    await waitFor(() => expect(screen.queryByRole('tooltip')).toBeNull());

    await user.click(descriptionButton);
    expect((await screen.findByRole('tooltip')).textContent).toBe(description);
  });

  it('renders every chart plugin from production panel query frames', async () => {
    const chartTypes = [
      'time_series',
      'stat',
      'gauge',
      'bar_gauge',
      'bar_chart',
      'heatmap',
      'state_timeline',
    ] as const satisfies readonly VisualizationType[];
    const dashboard = createEmptyDashboardDefinition('Runtime charts');
    dashboard.refreshSettings = {
      enabled: false,
      mode: 'off',
      allowedIntervals: ['off'],
    };
    dashboard.elements = chartPanels(chartTypes);
    const executor = vi.fn(async (): Promise<DataFrame[]> => [chartFrame()]);
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });

    render(
      <MemoryRouter>
        <QueryClientProvider client={queryClient}>
          <DashboardRenderer
            dashboard={dashboard}
            orgId="org-1"
            panelQueryExecutor={executor}
          />
        </QueryClientProvider>
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(executor).toHaveBeenCalledTimes(chartTypes.length);
      expect(
        screen.getByRole('img', { name: 'Dashboard time series' }),
      ).toBeTruthy();
      expect(screen.getByRole('img', { name: 'value: 30' })).toBeTruthy();
      expect(
        screen.getByRole('img', { name: 'value: 30; 0–100' }),
      ).toBeTruthy();
      expect(screen.getByRole('meter', { name: 'value' })).toBeTruthy();
      expect(
        screen.getByRole('img', {
          name: 'Bar chart with 2 categories and 1 series',
        }),
      ).toBeTruthy();
      expect(
        screen.getByRole('img', {
          name: 'Heatmap with 1 series and 3 columns; values 10 to 30',
        }),
      ).toBeTruthy();
      expect(
        screen.getByRole('img', { name: /State timeline with 2 rows/ }),
      ).toBeTruthy();
    });
  });

  it('defaults crosshair sharing off and enables it from dashboard settings', async () => {
    const dashboard = createEmptyDashboardDefinition('Cursor sync');
    dashboard.refreshSettings = {
      enabled: false,
      mode: 'off',
      allowedIntervals: ['off'],
    };
    delete dashboard.interactionSettings;
    const [panel] = chartPanels(['time_series']);
    if (!panel) throw new Error('Expected a time-series panel');
    dashboard.elements = [panel];
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });
    const view = render(
      <MemoryRouter>
        <QueryClientProvider client={queryClient}>
          <DashboardRenderer
            dashboard={dashboard}
            orgId="org-1"
            panelQueryExecutor={async () => [chartFrame()]}
          />
        </QueryClientProvider>
      </MemoryRouter>,
    );

    expect(
      (await screen.findByTestId('time-series-chart')).getAttribute(
        'data-cursor-sync',
      ),
    ).toBe('off');

    view.rerender(
      <MemoryRouter>
        <QueryClientProvider client={queryClient}>
          <DashboardRenderer
            dashboard={{
              ...dashboard,
              interactionSettings: { cursorSync: 'shared_crosshair' },
            }}
            orgId="org-1"
            panelQueryExecutor={async () => [chartFrame()]}
          />
        </QueryClientProvider>
      </MemoryRouter>,
    );
    expect(
      screen
        .getByTestId('time-series-chart')
        .getAttribute('data-cursor-sync'),
    ).toBe('shared_crosshair');
  });

  it('keeps the live chart visible inside Dashboard edit controls', async () => {
    const dashboard = createEmptyDashboardDefinition('Editable chart');
    dashboard.refreshSettings = {
      enabled: false,
      mode: 'off',
      allowedIntervals: ['off'],
    };
    const [panel] = chartPanels(['stat']);
    dashboard.elements = panel ? [panel] : [];
    const executor = vi.fn(async (): Promise<DataFrame[]> => [chartFrame()]);
    const onSelectElement = vi.fn();
    const onInteractionStart = vi.fn();
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });

    render(
      <MemoryRouter>
        <QueryClientProvider client={queryClient}>
          <DashboardRenderer
            dashboard={dashboard}
            orgId="org-1"
            panelQueryExecutor={executor}
            editMode={{
              selectedIds: new Set(['panel-stat']),
              onSelectElement,
              onInteractionStart,
            }}
          />
        </QueryClientProvider>
      </MemoryRouter>,
    );

    const chart = await screen.findByRole('img', { name: 'value: 30' });
    const editItem = chart.closest<HTMLElement>(
      '[data-dashboard-edit-element]',
    );
    expect(editItem?.dataset.selected).toBe('true');

    fireEvent.click(editItem!);
    expect(onSelectElement).toHaveBeenCalledWith('panel-stat', false);

    expect(
      screen.queryByRole('button', { name: 'Drag stat' }),
    ).toBeNull();
    fireEvent.pointerDown(editItem!, {
      button: 0,
      pointerId: 7,
      clientX: 100,
      clientY: 50,
    });
    expect(onInteractionStart).toHaveBeenCalledWith(
      expect.anything(),
      'panel-stat',
      'move',
    );

    onInteractionStart.mockClear();
    fireEvent.pointerDown(
      screen.getByRole('button', { name: 'Open panel menu: stat' }),
      { button: 0, pointerId: 8 },
    );
    expect(onInteractionStart).not.toHaveBeenCalled();
    expect(
      screen.queryByRole('button', { name: 'Resize stat' }),
    ).toBeNull();
    expect(
      editItem?.querySelector('[data-dashboard-resize-handle]'),
    ).toBeTruthy();
  });

  it('keeps the last successful frame visible during background refresh without overlap', async () => {
    const dashboard = createEmptyDashboardDefinition('Background refresh');
    dashboard.refreshSettings = {
      enabled: false,
      mode: 'off',
      allowedIntervals: ['off'],
    };
    const [panel] = chartPanels(['time_series']);
    dashboard.elements = panel ? [panel] : [];
    let resolveRefresh: ((frames: DataFrame[]) => void) | undefined;
    const executor = vi
      .fn<DashboardPanelQueryExecutor>()
      .mockResolvedValueOnce([chartFrame()])
      .mockImplementationOnce(
        () =>
          new Promise<DataFrame[]>((resolve) => {
            resolveRefresh = resolve;
          }),
      );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });
    const view = render(
      <MemoryRouter>
        <QueryClientProvider client={queryClient}>
          <DashboardRenderer
            dashboard={dashboard}
            orgId="org-1"
            refreshIntervalOverride={40}
            panelQueryExecutor={executor}
          />
        </QueryClientProvider>
      </MemoryRouter>,
    );

    expect(
      await screen.findByRole('img', { name: 'Dashboard time series' }),
    ).toBeTruthy();
    await waitFor(() => expect(executor).toHaveBeenCalledTimes(2));
    expect(
      screen.getByRole('img', { name: 'Dashboard time series' }),
    ).toBeTruthy();
    expect(screen.queryByText('Loading visualization…')).toBeNull();
    expect(screen.getByRole('status', { name: 'Refreshing panel' })).toBeTruthy();

    await new Promise((resolve) => globalThis.setTimeout(resolve, 120));
    expect(executor).toHaveBeenCalledTimes(2);
    view.unmount();
    resolveRefresh?.([chartFrame()]);
  });

  it('keeps previous data while a changed time range is loading', async () => {
    const dashboard = createEmptyDashboardDefinition('Time range refresh');
    dashboard.refreshSettings = {
      enabled: false,
      mode: 'off',
      allowedIntervals: ['off'],
    };
    const [panel] = chartPanels(['time_series']);
    dashboard.elements = panel ? [panel] : [];
    let resolveRangeQuery: ((frames: DataFrame[]) => void) | undefined;
    const executor = vi
      .fn<DashboardPanelQueryExecutor>()
      .mockResolvedValueOnce([chartFrame()])
      .mockImplementationOnce(
        () =>
          new Promise<DataFrame[]>((resolve) => {
            resolveRangeQuery = resolve;
          }),
      );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });
    const view = render(
      <MemoryRouter>
        <QueryClientProvider client={queryClient}>
          <DashboardRenderer
            dashboard={dashboard}
            orgId="org-1"
            panelQueryExecutor={executor}
          />
        </QueryClientProvider>
      </MemoryRouter>,
    );

    expect(
      await screen.findByRole('img', { name: 'Dashboard time series' }),
    ).toBeTruthy();
    act(() => {
      useTimeStore.getState().setWindow({
        from: 'now-24h',
        to: 'now',
        mode: 'relative',
      });
    });
    await waitFor(() => expect(executor).toHaveBeenCalledTimes(2));
    expect(
      screen.getByRole('img', { name: 'Dashboard time series' }),
    ).toBeTruthy();
    expect(screen.queryByText('Loading visualization…')).toBeNull();

    view.unmount();
    resolveRangeQuery?.([chartFrame()]);
  });

  it('updates an edited query Legend from cached frames without rerunning the query', async () => {
    const dashboard = createEmptyDashboardDefinition('Live query Legend');
    dashboard.refreshSettings = {
      enabled: false,
      mode: 'off',
      allowedIntervals: ['off'],
    };
    const panel = createDashboardPanel([], 'time_series');
    panel.id = 'panel-live-legend';
    panel.title = 'Live Legend';
    panel.queries = panel.queries.map((query) => ({
      ...query,
      legend: 'Service {{ service }}',
    }));
    panel.visualization.options = {
      ...panel.visualization.options,
      legendMode: 'list',
      legendStats: [],
    };
    dashboard.elements = [panel];
    const executor = vi.fn(
      async (
        ...[, query]: Parameters<DashboardPanelQueryExecutor>
      ): Promise<DataFrame[]> => {
        expect(query.legend).toBeUndefined();
        return [legendFrame()];
      },
    );
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: 0 } },
    });
    const view = render(
      <MemoryRouter>
        <QueryClientProvider client={queryClient}>
          <DashboardRenderer
            dashboard={dashboard}
            orgId="org-1"
            panelQueryExecutor={executor}
          />
        </QueryClientProvider>
      </MemoryRouter>,
    );

    expect(
      await screen.findByRole('button', {
        name: 'Service checkout',
      }),
    ).toBeTruthy();
    expect(executor).toHaveBeenCalledTimes(1);

    const updatedDashboard = {
      ...dashboard,
      elements: [
        {
          ...panel,
          queries: panel.queries.map((query) => ({
            ...query,
            legend: '{{ service }} requests',
          })),
        },
      ],
    };
    view.rerender(
      <MemoryRouter>
        <QueryClientProvider client={queryClient}>
          <DashboardRenderer
            dashboard={updatedDashboard}
            orgId="org-1"
            panelQueryExecutor={executor}
          />
        </QueryClientProvider>
      </MemoryRouter>,
    );

    expect(
      await screen.findByRole('button', {
        name: 'checkout requests',
      }),
    ).toBeTruthy();
    expect(
      screen.queryByRole('button', { name: 'Service checkout' }),
    ).toBeNull();
    await waitFor(() => expect(executor).toHaveBeenCalledTimes(1));
  });
});

function chartPanels(
  types: readonly VisualizationType[],
): DashboardElement[] {
  const elements: DashboardElement[] = [];
  for (const [index, type] of types.entries()) {
    const panel = createDashboardPanel(elements, type);
    panel.id = `panel-${type}`;
    panel.title = type;
    panel.visualization.options = {};
    panel.gridPos = {
      ...panel.gridPos,
      x: (index % 2) * 12,
      y: Math.floor(index / 2) * 20,
      w: 12,
      h: 20,
    };
    elements.push(panel);
  }
  return elements;
}

function chartFrame(): DataFrame {
  return {
    refId: 'A',
    length: 3,
    fields: [
      {
        id: 'state',
        name: 'state',
        type: 'string',
        values: ['ready', 'ready', 'failed'],
      },
      {
        id: 'time',
        name: 'time',
        type: 'time',
        values: [1_700_000_000, 1_700_000_010, 1_700_000_030],
      },
      {
        id: 'value',
        name: 'value',
        type: 'number',
        values: [10, 20, 30],
        config: { min: 0, max: 100 },
      },
    ],
  };
}

function legendFrame(): DataFrame {
  return {
    refId: 'A',
    name: 'value',
    length: 2,
    fields: [
      {
        id: 'legend-time',
        name: 'time',
        type: 'time',
        values: [1_700_000_000, 1_700_000_010],
      },
      {
        id: 'legend-value',
        name: 'value',
        type: 'number',
        values: [10, 20],
        labels: { service: 'checkout' },
      },
    ],
  };
}
