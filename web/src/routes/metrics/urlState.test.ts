import { describe, expect, it } from 'vitest';

import { metricsStateFromParams, writeExecutedMetricsState } from './urlState';

describe('Metrics Explore URL state', () => {
  it('preserves investigation context while writing an executed query', () => {
    const result = writeExecutedMetricsState(
      new URLSearchParams('time=now-6h..now&filters=service%3Dapi'),
      {
        promql: 'rate(http_requests_total[5m])',
        queryOptions: {
          legend: '{{service}}',
          format: 'table',
          step: '30s',
          type: 'instant',
          exemplars: false,
        },
        drawStyle: 'bar',
        stackMode: 'percent',
      },
    );

    expect(result.get('time')).toBe('now-6h..now');
    expect(result.get('filters')).toBe('service=api');
    expect(result.get('promql')).toBe('rate(http_requests_total[5m])');
    expect(result.get('step')).toBe('30s');
    expect(result.get('view')).toBe('table');
    expect(result.get('type')).toBe('instant');
    expect(result.get('exemplars')).toBe('0');
    expect(result.get('draw')).toBe('bar');
    expect(result.get('stack_mode')).toBe('percent');
  });

  it('hydrates query and chart options from a shared URL', () => {
    const state = metricsStateFromParams(
      new URLSearchParams(
        'step=1m&legend=%7B%7Binstance%7D%7D&view=table&type=instant&exemplars=0&draw=points&stack_mode=normal',
      ),
    );

    expect(state).toEqual({
      queryOptions: {
        legend: '{{instance}}',
        format: 'table',
        step: '1m',
        type: 'instant',
        exemplars: false,
      },
      drawStyle: 'points',
      stackMode: 'normal',
    });
  });
});
