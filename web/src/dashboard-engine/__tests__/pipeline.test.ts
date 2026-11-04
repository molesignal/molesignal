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
        scannedRows: 14,
        tookMs: 3,
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
        scannedRows: 3,
        tookMs: 1,
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

  it('draws a SQL time series that is long as a line for each set of labels', () => {
    const result = {
      columns: ['time', 'browser.name', 'count'],
      rows: [
        [1, 'Chrome', 4],
        [1, 'Safari', 2],
        [2, 'Chrome', 7],
        [2, null, 1],
      ],
      scannedRows: 4,
      tookMs: 1,
    };
    const frames = queryResultToDataFrames(result, 'A', 'sql', undefined, 'time_series');

    expect(frames.map((frame) => frame.name)).toEqual(['Chrome', 'Safari', '—']);
    expect(frames[0]?.fields.map((field) => field.values)).toEqual([
      [1, 2],
      [4, 7],
    ]);
    expect(frames[0]?.fields[1]?.labels).toEqual({ 'browser.name': 'Chrome' });
    expect(frames[2]?.fields[1]?.labels).toEqual({});
    expect(new Set(frames.flatMap((frame) => frame.fields.map((field) => field.id))).size).toBe(6);

    // Without the format, or without a label to split by, a result stays one frame.
    expect(queryResultToDataFrames(result, 'A', 'sql')).toHaveLength(1);
    expect(
      queryResultToDataFrames(
        { columns: ['time', 'count'], rows: [[1, 4]], scannedRows: 1, tookMs: 1 },
        'A',
        'sql',
        undefined,
        'time_series',
      ),
    ).toHaveLength(1);
  });

  it('keeps legend formatting isolated between multiple queries', () => {
    const result = {
      columns: ['timestamp', 'service', 'value'],
      rows: [
        [1, 'checkout', 4],
        [1, 'gateway', 2],
      ],
      scannedRows: 2,
      tookMs: 1,
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
        scannedRows: 2,
        tookMs: 1,
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
