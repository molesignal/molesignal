import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import * as React from 'react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  SeriesIdentifier,
  type SeriesIdentityConfig,
} from './SeriesIdentifier';

afterEach(cleanup);

const text: SeriesIdentityConfig = {
  title: 'Series',
  countLabel: '1 series',
  nameLabel: 'Name',
  labelCountLabel: (count) => `${count} labels`,
  expandLabel: (metricName) => `Show labels for ${metricName}`,
  collapseLabel: (metricName) => `Hide labels for ${metricName}`,
};

function Harness({
  onSelect,
  legendName,
}: {
  onSelect: () => void;
  legendName?: string;
}) {
  const [expanded, setExpanded] = React.useState(false);
  return (
    <MemoryRouter>
      <SeriesIdentifier
        series={{
          metricName: 'cache_misses_total',
          ...(legendName !== undefined ? { legendName } : {}),
          name: legendName ?? 'cache_misses_total{service.name="molesignal"}',
          data: [0],
          labels: {
            __name__: 'cache_misses_total',
            'service.name': 'molesignal',
            'deployment.environment.name': 'production',
          },
        }}
        hidden={false}
        expanded={expanded}
        text={text}
        onSelect={onSelect}
        onExpandedChange={setExpanded}
        onFocusChange={() => undefined}
      />
    </MemoryRouter>
  );
}

describe('SeriesIdentifier', () => {
  it('keeps the metric name compact and reveals labels on demand', () => {
    const onSelect = vi.fn();
    render(<Harness onSelect={onSelect} />);

    const metric = screen.getByRole('button', { name: 'cache_misses_total' });
    expect(metric.textContent).toBe('cache_misses_total');
    expect(metric.className).toContain('font-sans');
    expect(metric.className).not.toContain('font-code');
    expect(screen.getByText('2 labels')).not.toBeNull();
    expect(screen.queryByTestId('series-identifier-labels')).toBeNull();

    fireEvent.click(
      screen.getByRole('button', {
        name: 'Show labels for cache_misses_total',
      }),
    );

    expect(screen.getByTestId('series-identifier-labels')).not.toBeNull();
    const labelName = screen.getByText('deployment.environment.name');
    const labelValue = screen.getByText('production');
    expect(labelName.className).toContain('font-sans');
    expect(labelValue.className).toContain('font-sans');
    expect(
      screen.getByRole('button', { name: 'molesignal' }).getAttribute('data-signal-type'),
    ).toBe('service');
    expect(screen.queryByText('__name__')).toBeNull();

    fireEvent.click(metric);
    expect(onSelect).toHaveBeenCalledOnce();
  });

  it('leads with the legend its author wrote, and still opens the labels', () => {
    const onSelect = vi.fn();
    render(<Harness onSelect={onSelect} legendName="production cache" />);

    // The written legend is the name of the row; the metric is not repeated.
    const named = screen.getByRole('button', { name: 'production cache' });
    expect(named.textContent).toBe('production cache');
    expect(
      screen.queryByRole('button', { name: 'cache_misses_total' }),
    ).toBeNull();
    expect(screen.getByText('2 labels')).not.toBeNull();

    fireEvent.click(
      screen.getByRole('button', {
        name: 'Show labels for production cache',
      }),
    );
    expect(screen.getByTestId('series-identifier-labels')).not.toBeNull();
    expect(screen.getByText('deployment.environment.name')).not.toBeNull();

    fireEvent.click(named);
    expect(onSelect).toHaveBeenCalledOnce();
  });

  it('falls back to the metric when the written legend renders to nothing', () => {
    render(<Harness onSelect={() => undefined} legendName="  " />);

    expect(
      screen.getByRole('button', { name: 'cache_misses_total' }).textContent,
    ).toBe('cache_misses_total');
  });
});
