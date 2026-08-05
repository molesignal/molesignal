import { describe, expect, it } from 'vitest';

import {
  queryResultToDataFrames,
  renderLegendFormat,
} from '../dataframe';
import { applyFieldConfig, formatFieldValue } from '../fieldConfig';
import { applyTransformations } from '../transformations';

describe('dashboard DataFrame pipeline', () => {
  it('adapts metric rows before visualization', () => {
    const frames = queryResultToDataFrames(
      {
        columns: ['timestamp', 'service', 'value'],
        rows: [
          [1, 'alpha', 4],
          [2, 'alpha', 7],
          [1, 'beta', 2],
        ],
        scanned_rows: 14,
        took_ms: 3,
      },
      'A',
      'metrics',
    );
    expect(frames).toHaveLength(2);
    expect(frames[0]?.fields.map((field) => field.type)).toEqual([
      'time',
      'number',
    ]);
    expect(frames[0]?.fields[1]?.labels).toEqual({ service: 'alpha' });
  });

  it('formats each metric series with its query legend template', () => {
    const frames = queryResultToDataFrames(
      {
        columns: ['timestamp', 'service', 'method', 'value'],
        rows: [
          [1, 'checkout', 'GET', 4],
          [2, 'checkout', 'GET', 7],
          [1, 'gateway', 'POST', 2],
        ],
        scanned_rows: 3,
        took_ms: 1,
      },
      'A',
      'metrics',
      '{{service}} · {{ method }}',
    );

    expect(frames.map((frame) => frame.name)).toEqual([
      'checkout · GET',
      'gateway · POST',
    ]);
  });

  it('keeps legend formatting isolated between multiple queries', () => {
    const result = {
      columns: ['timestamp', 'service', 'value'],
      rows: [
        [1, 'checkout', 4],
        [1, 'gateway', 2],
      ],
      scanned_rows: 2,
      took_ms: 1,
    };

    const serviceFrames = queryResultToDataFrames(
      result,
      'A',
      'metrics',
      '{{service}}',
    );
    const fixedFrames = queryResultToDataFrames(
      result,
      'B',
      'metrics',
      'Request rate',
    );

    expect(serviceFrames.map((frame) => frame.name)).toEqual([
      'checkout',
      'gateway',
    ]);
    expect(fixedFrames.map((frame) => frame.name)).toEqual([
      'Request rate',
      'Request rate',
    ]);
  });

  it('matches Grafana fallback behavior for missing legend labels', () => {
    expect(
      renderLegendFormat(
        '{{service}} / {{missing}}',
        { service: 'checkout' },
      ),
    ).toBe('checkout / missing');
  });

  it('applies transformations and field overrides in order', () => {
    const frames = queryResultToDataFrames(
      {
        columns: ['service', 'errors', 'requests'],
        rows: [
          ['a', 4, 10],
          ['b', 1, 20],
        ],
        scanned_rows: 2,
        took_ms: 1,
      },
      'A',
      'sql',
    );
    const transformed = applyTransformations(frames, [
      {
        id: 'calc',
        type: 'calculate_field',
        options: {
          alias: 'ratio',
          left: 'errors',
          right: 'requests',
          operation: 'divide',
        },
      },
      {
        id: 'sort',
        type: 'sort_by',
        options: { field: 'ratio', direction: 'desc' },
      },
      { id: 'limit', type: 'limit', options: { count: 1 } },
    ]);
    const configured = applyFieldConfig(
      transformed,
      { decimals: 2 },
      [
        {
          id: 'ratio-unit',
          matcher: { type: 'field_name', value: 'ratio' },
          properties: [{ id: 'unit', value: 'percent' }],
        },
      ],
    );
    const ratio = configured[0]?.fields.find(
      (field) => field.name === 'ratio',
    );
    expect(ratio?.values).toEqual([0.4]);
    expect(ratio?.config?.unit).toBe('percent');
    expect(formatFieldValue(0.4, ratio?.config).text).toBe('0.40%');
  });
});
