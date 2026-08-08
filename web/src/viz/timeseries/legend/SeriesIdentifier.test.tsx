import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import * as React from 'react';
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

function Harness({ onSelect }: { onSelect: () => void }) {
  const [expanded, setExpanded] = React.useState(false);
  return (
    <SeriesIdentifier
      series={{
        metricName: 'cache_misses_total',
        name: 'cache_misses_total{service.name="molesignal"}',
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
    expect(screen.queryByText('__name__')).toBeNull();

    fireEvent.click(metric);
    expect(onSelect).toHaveBeenCalledOnce();
  });
});
