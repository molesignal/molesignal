import { describe, expect, it } from 'vitest';

import type { DataFrame, PanelQuery } from '../schema';
import {
  applyQueryPresentation,
  toExecutablePanelQuery,
} from './presentation';

describe('dashboard query presentation', () => {
  it('keeps Legend out of the executable query identity', () => {
    const query = metricQuery('{{ service }}');

    expect(toExecutablePanelQuery(query)).toEqual({
      refId: 'A',
      enabled: true,
      dataSourceType: 'metrics',
      query: { language: 'promql', expression: 'up' },
    });
    expect(query.legend).toBe('{{ service }}');
  });

  it('relabels cached metric frames from their series labels', () => {
    const frame = metricFrame();

    const [presented] = applyQueryPresentation(
      [frame],
      metricQuery('Service {{ service }}'),
    );

    expect(presented?.name).toBe('Service checkout');
    expect(frame.name).toBe('value');
  });

  it('uses only labels that differ between series in Auto mode', () => {
    const frames = [
      metricFrame({ env: 'prod', service: 'checkout' }),
      metricFrame({ env: 'prod', service: 'gateway' }),
    ];

    expect(
      applyQueryPresentation(frames, metricQuery('__auto')).map(
        (frame) => frame.name,
      ),
    ).toEqual(['{service="checkout"}', '{service="gateway"}']);
  });

  it('uses every label name and value in legacy Verbose mode', () => {
    const frames = [metricFrame({ env: 'prod', service: 'checkout' })];

    expect(
      applyQueryPresentation(frames, metricQuery(undefined))[0]?.name,
    ).toBe('{env="prod", service="checkout"}');
  });
});

function metricQuery(legend: string | undefined): PanelQuery {
  return {
    refId: 'A',
    enabled: true,
    dataSourceType: 'metrics',
    query: { language: 'promql', expression: 'up' },
    ...(legend === undefined ? {} : { legend }),
  };
}

function metricFrame(
  labels: Record<string, string> = { service: 'checkout' },
): DataFrame {
  return {
    refId: 'A',
    name: 'value',
    length: 2,
    fields: [
      {
        id: 'time',
        name: 'time',
        type: 'time',
        values: [1, 2],
      },
      {
        id: 'value',
        name: 'value',
        type: 'number',
        values: [1, 2],
        labels,
      },
    ],
  };
}
