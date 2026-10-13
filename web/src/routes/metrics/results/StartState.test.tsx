import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { MetricCatalogEntry } from '@/api/metricsCatalog';
import i18n from '@/i18n';

import { MetricsStartState } from './StartState';

function metric(name: string, metric_type: MetricCatalogEntry['metric_type'] = 'gauge'): MetricCatalogEntry {
  return { name, metric_type, labels: ['service'], field_count: 3 } as MetricCatalogEntry;
}

function renderStart(metrics: MetricCatalogEntry[], pending = false) {
  const handlers = { onPick: vi.fn(), onBrowse: vi.fn() };
  render(
    <MemoryRouter>
      <MetricsStartState starter={{ metrics, pending, ...handlers }} />
    </MemoryRouter>,
  );
  return handlers;
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

afterEach(cleanup);

describe('MetricsStartState', () => {
  it('prompts for a query and offers the metrics the organization already has', () => {
    renderStart([metric('http_requests_total', 'counter'), metric('queue_depth')]);

    expect(screen.getByRole('status').textContent).toContain('PromQL');
    expect(screen.getByRole('heading', { level: 2, name: 'Start from a metric' })).not.toBeNull();
    expect(screen.getByRole('button', { name: 'Chart http_requests_total' })).not.toBeNull();
    expect(screen.getByRole('button', { name: 'Chart queue_depth' }).textContent).toContain(
      '1 labels · 3 fields',
    );
  });

  it('charts a metric on one click', () => {
    const picked = metric('queue_depth');
    const { onPick } = renderStart([metric('http_requests_total', 'counter'), picked]);

    fireEvent.click(screen.getByRole('button', { name: 'Chart queue_depth' }));

    expect(onPick).toHaveBeenCalledWith(picked);
  });

  it('offers a handful to start from and the browser for the rest', () => {
    const many = Array.from({ length: 12 }, (_, index) => metric(`metric_${index}`));
    const { onBrowse } = renderStart(many);

    expect(screen.getAllByRole('button', { name: /^Chart / })).toHaveLength(8);

    fireEvent.click(screen.getByRole('button', { name: 'Browse all metrics' }));
    expect(onBrowse).toHaveBeenCalledTimes(1);
  });

  it('holds the place of the starters while the catalog loads', () => {
    renderStart([], true);

    expect(screen.queryByRole('button', { name: /^Chart / })).toBeNull();
    expect(screen.queryByText('No metrics have arrived yet.')).toBeNull();
    expect(screen.getByRole('status')).not.toBeNull();
  });

  it('points at a way to send metrics when there are none', () => {
    renderStart([]);

    expect(screen.getByText('No metrics have arrived yet.')).not.toBeNull();
    expect(
      screen.getByRole('link', { name: 'Connect a metrics source' }).getAttribute('href'),
    ).toBe('/datasource/applications/opentelemetry?signal=metrics');
  });
});
