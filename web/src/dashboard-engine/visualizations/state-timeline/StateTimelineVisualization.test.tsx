import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { createDashboardPanel } from '../../factories';
import type { DataField, DataFrame, PanelData } from '../../schema';
import { prepareStateTimeline } from './model';
import { StateTimelineVisualization } from './StateTimelineVisualization';

afterEach(cleanup);

describe('StateTimelineVisualization', () => {
  it('uses irregular timestamps as real segment durations and a median final interval', () => {
    const start = 1_700_000_000_000;
    const model = prepareStateTimeline(
      [
        frame([
          field('time', 'time', [start, start + 10_000, start + 40_000]),
          field('state', 'string', ['starting', 'ready', 'failed']),
        ]),
      ],
      false,
    )!;
    const segments = model.rows[0]!.segments;

    expect(model.usesTime).toBe(true);
    expect(segments.map((segment) => segment.end - segment.start)).toEqual([
      10,
      30,
      20,
    ]);
    expect(model.end - model.start).toBe(60);
  });

  it('merges adjacent states by formatted text and color', () => {
    const state = field('state', 'string', ['ok', 'healthy', 'bad']);
    state.config = {
      mappings: [
        { type: 'value', value: 'ok', result: { text: 'Healthy', color: 'var(--green)' } },
        { type: 'value', value: 'healthy', result: { text: 'Healthy', color: 'var(--green)' } },
        { type: 'value', value: 'bad', result: { text: 'Failed', color: 'var(--red)' } },
      ],
    };
    const model = prepareStateTimeline(
      [frame([field('time', 'time', [0, 10, 20]), state])],
      true,
    )!;

    expect(model.rows[0]?.segments).toHaveLength(2);
    expect(model.rows[0]?.segments[0]).toMatchObject({
      start: 0,
      end: 20,
      text: 'Healthy',
      color: 'var(--green)',
    });
  });

  it('falls back to index positions and limits the legend to eight states', () => {
    const values = Array.from({ length: 10 }, (_, index) => `state-${index}`);
    const model = prepareStateTimeline(
      [frame([field('state', 'string', values)])],
      false,
    )!;

    expect(model.usesTime).toBe(false);
    expect(model.start).toBe(0);
    expect(model.end).toBe(10);
    expect(model.legend).toHaveLength(8);
    expect(model.legendTruncated).toBe(true);
  });

  it('renders proportional segments, auto labels, titles, and an accessible summary', () => {
    renderTimeline(
      frame([
        field('time', 'time', [0, 10, 40]),
        field('state', 'string', ['ready', 'ready', 'failed']),
      ]),
      { mergeEqual: true, showValues: 'auto' },
    );

    expect(
      screen.getByRole('img', { name: 'State timeline with 1 rows and 2 states' }),
    ).toBeTruthy();
    const segments = screen.getAllByTestId('state-timeline-segment');
    expect(segments).toHaveLength(2);
    expect(segments[0]?.getAttribute('style')).toContain('width: 66.666');
    expect(segments[0]?.getAttribute('title')).toContain('ready');
    expect(screen.getAllByTestId('state-timeline-label')).toHaveLength(2);
  });

  it('obeys never-label mode and renders an empty state without rows', () => {
    const { rerender } = renderTimeline(
      frame([field('state', 'boolean', [true, false])]),
      { showValues: 'never' },
    );
    expect(screen.queryByTestId('state-timeline-label')).toBeNull();

    const data: PanelData = {
      state: 'done',
      frames: [frame([field('time', 'time', [0, 1])])],
      timeRange: { from: 0, to: 1 },
    };
    rerender(
      <StateTimelineVisualization
        panel={createDashboardPanel([], 'state_timeline')}
        data={data}
        options={{}}
        height={180}
      />,
    );
    expect(screen.getByText('No data')).toBeTruthy();
  });
});

function renderTimeline(value: DataFrame, options: Record<string, unknown>) {
  const data: PanelData = {
    state: 'done',
    frames: [value],
    timeRange: { from: 0, to: 1 },
  };
  return render(
    <StateTimelineVisualization
      panel={createDashboardPanel([], 'state_timeline')}
      data={data}
      options={options}
      height={180}
    />,
  );
}

function frame(fields: DataField[]): DataFrame {
  return {
    refId: 'A',
    length: Math.max(0, ...fields.map((item) => item.values.length)),
    fields,
  };
}

function field(id: string, type: DataField['type'], values: unknown[]): DataField {
  return { id, name: id, type, values };
}
