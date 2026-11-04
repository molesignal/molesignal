import { describe, expect, it } from 'vitest';

import {
  builderFunctionOptions,
  composeBuilderPromql,
  parseBuilderPromql,
  type BuilderQuery,
} from './model';

describe('PromQL builder model', () => {
  it('composes nested transform, aggregation, and label matchers', () => {
    const query: BuilderQuery = {
      metric: 'http_requests_total',
      transform: 'rate',
      aggregation: 'sum',
      range: '5m',
      matchers: [
        {
          id: 'matcher-0',
          name: 'service_name',
          operator: '=~',
          value: 'api-"primary"',
        },
      ],
    };

    expect(composeBuilderPromql(query)).toBe(
      'sum(rate(http_requests_total{service_name=~"api-\\"primary\\""}[5m]))',
    );
  });

  it('parses a composed query back into builder controls', () => {
    expect(
      parseBuilderPromql(
        'avg(increase(job_events_total{service_name="worker",status!="ok"}[15m]))',
      ),
    ).toMatchObject({
      metric: 'job_events_total',
      transform: 'increase',
      aggregation: 'avg',
      range: '15m',
      matchers: [
        { name: 'service_name', operator: '=', value: 'worker' },
        { name: 'status', operator: '!=', value: 'ok' },
      ],
    });
  });

  it('quotes OTLP names with dots and reads them back', () => {
    const query: BuilderQuery = {
      metric: 'http.server.duration',
      transform: 'none',
      aggregation: 'sum',
      range: '5m',
      matchers: [
        { id: 'matcher-0', name: 'service.name', operator: '=', value: 'checkout' },
      ],
    };

    const promql = composeBuilderPromql(query);
    expect(promql).toBe('sum({"http.server.duration","service.name"="checkout"})');

    expect(parseBuilderPromql(promql)).toMatchObject({
      metric: 'http.server.duration',
      aggregation: 'sum',
      matchers: [{ name: 'service.name', operator: '=', value: 'checkout' }],
    });
  });

  it('rejects expressions that cannot be represented safely', () => {
    expect(parseBuilderPromql('sum by (service) (rate(foo_total[5m]))')).toBeNull();
  });

  it('uses backend capabilities for simple vector and range functions', () => {
    const functions = builderFunctionOptions([
      {
        label: 'avg_over_time',
        insertText: 'avg_over_time(${1:metric}[${2:5m}])',
        detail: 'avg_over_time(range-vector)',
        documentation: 'Average over a range.',
        kind: 'function',
      },
      {
        label: 'abs',
        insertText: 'abs(${1:vector})',
        detail: 'abs(vector)',
        documentation: 'Absolute value.',
        kind: 'function',
      },
      {
        label: 'histogram_quantile',
        insertText: 'histogram_quantile(${1:0.95}, ${2:vector})',
        detail: 'histogram_quantile(q, vector)',
        documentation: 'Needs an additional scalar argument.',
        kind: 'function',
      },
    ]);

    expect(functions.map(({ name, input }) => ({ name, input }))).toEqual([
      { name: 'none', input: 'vector' },
      { name: 'avg_over_time', input: 'range' },
      { name: 'abs', input: 'vector' },
      { name: 'histogram_quantile', input: null },
    ]);
    expect(
      composeBuilderPromql(
        {
          metric: 'queue_depth',
          transform: 'avg_over_time',
          aggregation: 'none',
          range: '15m',
          matchers: [],
        },
        functions,
      ),
    ).toBe('avg_over_time(queue_depth[15m])');
    expect(parseBuilderPromql('abs(queue_depth)', functions)).toMatchObject({
      metric: 'queue_depth',
      transform: 'abs',
    });
  });
});
