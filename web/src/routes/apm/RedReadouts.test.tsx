import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import type { RedSummary, TrendPoint } from '@/api/apm';
import i18n from '@/i18n';
import { reduceMotionInTests } from '@/shell/readout/reduceMotionInTests';

import { RedKpis } from './RedReadouts';

const START = Date.UTC(2026, 6, 30, 12) * 1_000;

function red(overrides: Partial<RedSummary> = {}): RedSummary {
  return {
    request_count: 84,
    error_count: 0,
    error_rate: 0,
    duration_sum_micros: 0,
    duration_average_micros: 180_000,
    p50_micros: 100_000,
    p95_micros: 250_000,
    p99_micros: 500_000,
    latency_partial: false,
    exemplars: [],
    ...overrides,
  };
}

const TREND: TrendPoint[] = [24, 48, 12].map((requests, index) => ({
  bucket_at: START + (index + 1) * 60_000_000,
  red: red({ request_count: requests, error_rate: index === 1 ? 0.25 : 0 }),
}));

function readout(label: string): HTMLElement {
  const cell = screen.getByText(label).closest<HTMLElement>('[data-readout]');
  if (!cell) throw new Error(`no readout labelled ${label}`);
  return cell;
}

let restoreMotion = () => {};

beforeEach(async () => {
  restoreMotion = reduceMotionInTests();
  await i18n.changeLanguage('en-us');
});

afterEach(() => {
  cleanup();
  restoreMotion();
});

describe('RedKpis', () => {
  it('reads throughput over the observed buckets and keeps the request count as context', () => {
    render(<RedKpis red={red()} trend={TREND} resolution="minute" />);

    const throughput = readout('Request throughput');
    // 84 requests over the three observed minutes.
    expect(throughput.textContent).toContain('0.47');
    expect(throughput.textContent).toContain('req/s');
    expect(throughput.textContent).toContain('84 total requests');
  });

  it('gives every figure the shape it had over the window', () => {
    render(<RedKpis red={red()} trend={TREND} resolution="minute" />);

    expect(readout('Request throughput').querySelector('[data-sparkline="bars"]')).not.toBeNull();
    for (const label of ['Error rate', 'P95 latency', 'P99 latency']) {
      expect(readout(label).querySelector('[data-sparkline="line"]')).not.toBeNull();
    }
  });

  it('colours the error rate by how bad it is', () => {
    const { rerender } = render(<RedKpis red={red({ error_rate: 0.004 })} trend={TREND} resolution="minute" />);
    expect(readout('Error rate').querySelector('.text-green')).not.toBeNull();

    rerender(<RedKpis red={red({ error_rate: 0.05 })} trend={TREND} resolution="minute" />);
    expect(readout('Error rate').querySelector('.text-yellow')).not.toBeNull();

    rerender(<RedKpis red={red({ error_rate: 0.2 })} trend={TREND} resolution="minute" />);
    expect(readout('Error rate').querySelector('.text-red')).not.toBeNull();
  });

  it('puts the median beside P95 and the average beside P99', () => {
    render(<RedKpis red={red()} trend={TREND} resolution="minute" />);

    expect(readout('P95 latency').textContent).toContain('P50 latency 100 ms');
    expect(readout('P99 latency').textContent).toContain('Average latency 180 ms');
  });

  it('holds its place with a baseline when the window has no buckets', () => {
    render(<RedKpis red={red({ request_count: 0 })} trend={[]} resolution="minute" />);

    expect(document.querySelectorAll('[data-sparkline="empty"]')).toHaveLength(4);
  });
});
