import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import * as React from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { MetricCatalogEntry } from '@/api/metricsCatalog';
import i18n from '@/i18n';

import { MetricCatalogPanel } from './MetricCatalogPanel';

const METRICS: MetricCatalogEntry[] = Array.from({ length: 45 }, (_, index) => ({
  name: `metric_${String(index).padStart(2, '0')}`,
  metric_type: 'gauge',
  labels: ['service.name'],
  field_count: 3,
}));

beforeEach(async () => {
  await i18n.changeLanguage('zh-cn');
});

afterEach(() => {
  cleanup();
});

function Harness() {
  const [filter, setFilter] = React.useState('');
  const [open, setOpen] = React.useState(true);
  return (
    <MetricCatalogPanel
      metrics={METRICS.slice(0, 20)}
      pending={false}
      error={null}
      selectedMetricName={null}
      filter={filter}
      open={open}
      pageSize={20}
      hasPrevious={false}
      hasNext
      onFilterChange={setFilter}
      onOpenChange={setOpen}
      onPickMetric={vi.fn()}
      onPrevious={vi.fn()}
      onNext={vi.fn()}
      onPageSizeChange={vi.fn()}
    />
  );
}

describe('MetricCatalogPanel', () => {
  it('renders a server page and exposes cursor navigation', () => {
    render(<Harness />);

    expect(screen.getByRole('dialog')).not.toBeNull();
    expect(screen.getByTestId('metrics-browser-dialog')).not.toBeNull();
    expect(screen.getByText('指标浏览器')).not.toBeNull();
    expect(screen.getByText('20 个结果')).not.toBeNull();
    expect(screen.getAllByRole('listitem')).toHaveLength(20);
    expect(screen.getByRole('button', { name: '上一页' })).toHaveProperty(
      'disabled',
      true,
    );
    expect(screen.getByRole('button', { name: '下一页' })).toHaveProperty(
      'disabled',
      false,
    );

    fireEvent.change(screen.getByPlaceholderText('过滤指标…'), {
      target: { value: 'metric_44' },
    });
    expect(screen.getByDisplayValue('metric_44')).not.toBeNull();
  });
});
