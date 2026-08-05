import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it } from 'vitest';

import type { PrometheusExemplarSeries } from '@/api/query';
import i18n from '@/i18n';

import {
  ExemplarRail,
  flattenPrometheusExemplars,
} from './ExemplarRail';

const series: PrometheusExemplarSeries[] = [
  {
    seriesLabels: {
      __name__: 'http_request_duration_seconds_bucket',
      service: 'checkout',
    },
    exemplars: [
      {
        labels: {
          trace_id: '0af7651916cd43dd8448eb211c80319c',
          span_id: 'b7ad6b7169203331',
        },
        value: 0.875,
        timestamp: 1_700_000_000,
      },
    ],
  },
];

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

describe('ExemplarRail', () => {
  it('normalizes native Prometheus timestamps and trace labels', () => {
    expect(flattenPrometheusExemplars(series)).toEqual([
      expect.objectContaining({
        timestampMicros: 1_700_000_000_000_000,
        traceId: '0af7651916cd43dd8448eb211c80319c',
        spanId: 'b7ad6b7169203331',
      }),
    ]);
  });

  it('links a chart marker to the retained trace', () => {
    render(
      <MemoryRouter>
        <ExemplarRail
          series={series}
          fromMicros={1_699_999_000_000_000}
          toMicros={1_700_001_000_000_000}
        />
      </MemoryRouter>,
    );

    expect(screen.getByTestId('metrics-exemplar-rail')).toBeTruthy();
    const link = screen.getByRole('link', {
        name: /0af7651916cd43dd8448eb211c80319c/,
      });
    expect(link.getAttribute('href')).toBe(
      '/traces/0af7651916cd43dd8448eb211c80319c',
    );
  });
});
