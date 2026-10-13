import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import type { RedSummary } from '@/api/apm';
import i18n from '@/i18n';

import { ApmPulse, healthVerdict, type ServiceHealthCounts } from './Pulse';

const RED: RedSummary = {
  request_count: 1_200,
  error_count: 12,
  error_rate: 0.01,
  duration_sum_micros: 0,
  duration_average_micros: 180_000,
  p50_micros: 100_000,
  p95_micros: 250_000,
  p99_micros: 500_000,
  latency_partial: false,
  exemplars: [],
};

function health(overrides: Partial<ServiceHealthCounts> = {}): ServiceHealthCounts {
  return { healthy: 0, warning: 0, critical: 0, no_traffic: 0, ...overrides };
}

function renderPulse(counts: ServiceHealthCounts) {
  return render(<ApmPulse red={RED} trend={[]} health={counts} resolution="minute" />);
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

afterEach(cleanup);

describe('healthVerdict', () => {
  it('is the worst state any service is in', () => {
    expect(healthVerdict(health({ healthy: 9, warning: 2, critical: 1 }))).toBe('critical');
    expect(healthVerdict(health({ healthy: 9, warning: 2 }))).toBe('warning');
    expect(healthVerdict(health({ healthy: 9, no_traffic: 3 }))).toBe('healthy');
    expect(healthVerdict(health({ no_traffic: 3 }))).toBe('idle');
    expect(healthVerdict(health())).toBe('idle');
  });
});

describe('ApmPulse', () => {
  it('opens with the verdict and how many services are well', () => {
    renderPulse(health({ healthy: 4, warning: 1, critical: 2, no_traffic: 1 }));

    const pulse = screen.getByRole('region', { name: 'Service health' });
    expect(pulse.getAttribute('data-verdict')).toBe('critical');
    expect(screen.getByRole('heading', { level: 2 }).textContent).toBe('2 services are critical');
    expect(screen.getByText('4 of 8 services healthy')).not.toBeNull();
  });

  it('speaks of one service in the singular', () => {
    renderPulse(health({ healthy: 4, warning: 1 }));

    expect(screen.getByRole('heading', { level: 2 }).textContent).toBe('1 service needs attention');
  });

  it('says so when everything is well, or when nothing is being served', () => {
    const { unmount } = renderPulse(health({ healthy: 6 }));
    expect(screen.getByRole('heading', { level: 2 }).textContent).toBe('All services are healthy');
    unmount();

    renderPulse(health({ no_traffic: 2 }));
    expect(screen.getByRole('heading', { level: 2 }).textContent).toBe('No traffic in this window');
  });

  it('draws the spread of health and names every state, present or not', () => {
    renderPulse(health({ healthy: 4, warning: 1 }));

    const bar = screen.getByRole('img', { name: /^Service health: / });
    expect(bar.getAttribute('aria-label')).toBe(
      'Service health: Healthy 4, Warning 1, Critical 0, No traffic 0',
    );
    // Only states that have services take room in the bar.
    expect(
      Array.from(bar.querySelectorAll('[data-spectrum-segment]')).map((segment) =>
        segment.getAttribute('data-spectrum-segment'),
      ),
    ).toEqual(['healthy', 'warning']);
  });

  it('carries the four RED figures beside the verdict', () => {
    renderPulse(health({ healthy: 4 }));

    for (const label of ['Request throughput', 'Error rate', 'P95 latency', 'P99 latency']) {
      expect(screen.getByText(label).closest('[data-readout]')).not.toBeNull();
    }
  });
});
