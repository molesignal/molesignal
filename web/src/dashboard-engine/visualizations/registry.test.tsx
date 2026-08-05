import '@/i18n';

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import * as React from 'react';
import { afterEach, describe, expect, it } from 'vitest';

import { createDashboardPanel } from '../factories';
import type { PanelData, VisualizationType } from '../schema';
import {
  visualizationRegistry,
  VisualizationRenderer,
} from '../visualizations';
import {
  OPTION_CHOICES,
  VisualizationJsonEditor,
} from './editor/VisualizationJsonEditor';

afterEach(cleanup);

describe('dashboard chart registry', () => {
  it('keeps the persisted schema version and exposes complete chart defaults', () => {
    expect(visualizationRegistry.get('stat')).toMatchObject({
      optionSchemaVersion: 1,
      defaultOptions: {
        calculation: 'last',
        textMode: 'value_and_name',
        graphMode: 'none',
        colorMode: 'value',
        showPercentChange: false,
      },
    });
    expect(visualizationRegistry.get('bar_gauge').defaultOptions).toMatchObject({
      orientation: 'horizontal',
      calculation: 'last',
      displayMode: 'basic',
      showThresholdMarkers: true,
    });
    expect(visualizationRegistry.get('bar_chart').defaultOptions).toMatchObject({
      orientation: 'vertical',
      groupWidth: 0.7,
      calculation: 'last',
      showValues: 'auto',
    });
    expect(visualizationRegistry.get('heatmap').defaultOptions).toEqual({
      colorScheme: 'blues',
    });
    expect(visualizationRegistry.get('state_timeline').defaultOptions).toEqual({
      mergeEqual: true,
      showValues: 'auto',
    });
  });

  it('documents every new generic editor mode including the avg alias', () => {
    expect(OPTION_CHOICES.calculation).toContain('avg');
    expect(OPTION_CHOICES.graphMode).toEqual(['none', 'area']);
    expect(OPTION_CHOICES.colorMode).toEqual(['none', 'value', 'background']);
    expect(OPTION_CHOICES.displayMode).toEqual(['basic', 'thresholds']);
    expect(OPTION_CHOICES.legendMode).toEqual(['list', 'table', 'hidden']);
  });

  it('passes time-series legend mode, placement, and calculations to the chart', () => {
    const base = createDashboardPanel([], 'time_series');
    const panel = {
      ...base,
      visualization: {
        ...base.visualization,
        options: {
          legendMode: 'table',
          legendPlacement: 'right',
          legendStats: ['sum'],
        },
      },
    };

    render(
      <VisualizationRenderer panel={panel} data={panelData()} height={180} />,
    );

    expect(
      screen
        .getByTestId('time-series-legend')
        .getAttribute('data-legend-placement'),
    ).toBe('right');
    expect(screen.getByRole('columnheader', { name: 'Total' })).toBeTruthy();
    expect(screen.queryByRole('columnheader', { name: 'Min' })).toBeNull();
  });

  it('updates live legend columns when the values picker changes', () => {
    render(<LegendValuesPreview />);

    expect(screen.getByRole('columnheader', { name: 'Last' })).toBeTruthy();
    expect(screen.queryByRole('columnheader', { name: 'Min' })).toBeNull();

    fireEvent.click(screen.getByRole('combobox', { name: /Legend values/ }));
    fireEvent.click(screen.getByRole('option', { name: 'Min' }));

    expect(screen.getByRole('columnheader', { name: 'Last' })).toBeTruthy();
    expect(screen.getByRole('columnheader', { name: 'Min' })).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Remove Last' }));
    expect(screen.queryByRole('columnheader', { name: 'Last' })).toBeNull();
    expect(screen.getByRole('columnheader', { name: 'Min' })).toBeTruthy();
  });

  it('shows production loading and error states before rendering empty charts', () => {
    const panel = createDashboardPanel([], 'stat');
    const loading: PanelData = {
      state: 'loading',
      frames: [],
      timeRange: { from: 0, to: 1 },
    };
    const { rerender } = render(
      <VisualizationRenderer panel={panel} data={loading} height={180} />,
    );
    expect(screen.getByRole('status').textContent).toContain(
      'Loading visualization…',
    );

    rerender(
      <VisualizationRenderer
        panel={panel}
        data={{ ...loading, state: 'streaming' }}
        height={180}
      />,
    );
    expect(screen.queryByText('Loading visualization…')).toBeNull();

    rerender(
      <VisualizationRenderer
        panel={panel}
        data={{
          ...loading,
          state: 'error',
          error: { message: 'query timed out' },
        }}
        height={180}
      />,
    );
    expect(screen.getByRole('alert').textContent).toContain(
      'Unable to load visualization',
    );
    expect(screen.getByRole('alert').textContent).toContain('query timed out');
  });

  it('keeps rendering available frames during a refresh', () => {
    const panel = createDashboardPanel([], 'stat');
    render(
      <VisualizationRenderer
        panel={panel}
        data={{ ...panelData(), state: 'loading' }}
        height={180}
      />,
    );
    expect(screen.getByRole('img', { name: 'value: 30' })).toBeTruthy();
    expect(screen.queryByRole('status')).toBeNull();
  });

  for (const type of [
    'stat',
    'gauge',
    'bar_gauge',
    'bar_chart',
    'heatmap',
    'state_timeline',
  ] as const satisfies readonly VisualizationType[]) {
    it(`renders the registered ${type} component`, () => {
      const panel = createDashboardPanel([], type);
      render(<VisualizationRenderer panel={panel} data={panelData()} height={180} />);

      if (type === 'bar_gauge') {
        expect(screen.getByRole('meter', { name: 'value' })).toBeTruthy();
      } else {
        expect(screen.getByRole('img')).toBeTruthy();
      }
    });
  }
});

function LegendValuesPreview() {
  const [options, setOptions] = React.useState<Record<string, unknown>>({
    legendMode: 'table',
    legendPlacement: 'bottom',
    legendStats: ['last'],
  });
  const base = createDashboardPanel([], 'time_series');
  const panel = {
    ...base,
    visualization: { ...base.visualization, options },
  };

  return (
    <>
      <VisualizationJsonEditor options={options} onChange={setOptions} />
      <VisualizationRenderer panel={panel} data={panelData()} height={180} />
    </>
  );
}

function panelData(): PanelData {
  return {
    state: 'done',
    frames: [
      {
        refId: 'A',
        length: 3,
        fields: [
          { id: 'time', name: 'time', type: 'time', values: [0, 10, 20] },
          {
            id: 'state',
            name: 'state',
            type: 'string',
            values: ['ready', 'ready', 'failed'],
          },
          {
            id: 'value',
            name: 'value',
            type: 'number',
            values: [10, 20, 30],
            config: { min: 0, max: 100 },
          },
        ],
      },
    ],
    timeRange: { from: 0, to: 30 },
  };
}
