import '@/i18n';

import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import type { ReactNode } from 'react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { Span, TraceResponse } from '@/api/web';
import { TooltipProvider } from '@/shell/ui/tooltip';
import { formatTraceDurationMs, formatTraceDurationNs } from '@/viz/trace/duration';
import { TraceFlame } from '@/viz/trace/TraceFlame';

const mocks = vi.hoisted(() => ({
  useTrace: vi.fn(),
}));

vi.mock('@/viz/trace/loader', () => ({
  useTrace: mocks.useTrace,
}));

vi.mock('@/viz/timeseries/themeAdapter', () => ({
  useThemePalette: () => ({
    palette: {
      '--accent': '#4f46e5',
      '--bg': '#ffffff',
      '--blue': '#2563eb',
      '--border': '#d1d5db',
      '--fg': '#111827',
      '--fg-muted': '#6b7280',
      '--green': '#15803d',
      '--primary': '#4f46e5',
      '--purple': '#7e22ce',
      '--red': '#dc2626',
      '--surface': '#ffffff',
      '--surface-muted': '#f3f4f6',
      '--yellow': '#a16207',
    },
    version: 0,
  }),
}));

afterEach(() => cleanup());

function span(
  spanId: string,
  parentSpanId: string | undefined,
  operation: string,
  service: string,
  startMs: number,
  durationMs: number,
): Span {
  return {
    span_id: spanId,
    ...(parentSpanId ? { parent_span_id: parentSpanId } : {}),
    operation,
    service,
    start_ns: startMs * 1_000_000,
    end_ns: (startMs + durationMs) * 1_000_000,
    duration_ns: durationMs * 1_000_000,
    kind: 1,
    status: 'OK',
    trace_flags: 0,
    trace_state: '',
    resource: { attributes: {}, dropped_attributes_count: 0 },
    scope: { name: '', version: '', attributes: {}, dropped_attributes_count: 0 },
    attributes: {},
    events: [],
    links: [],
    dropped_attributes_count: 0,
    dropped_events_count: 0,
    dropped_links_count: 0,
    schema_version: 1,
    semantic_conventions_version: '1.43.0',
    sampling_reason: 'default_ratio',
    partial: false,
    partial_reasons: [],
    late: false,
    duplicate: false,
    conflict: false,
  };
}

const trace: TraceResponse = {
  trace_id: 'trace-checkout-1',
  root_span_id: 'root',
  spans: [
    span('root', undefined, 'POST /api/checkout', 'api-gateway', 0, 184),
    span('auth', 'root', 'auth.verify', 'auth-service', 2, 25),
    span('checkout', 'root', 'checkout.create', 'checkout-service', 4, 165),
    span('payment', 'checkout', 'payment.authorize', 'payment-service', 6, 114),
    span('bank', 'payment', 'POST /v2/charges', 'bank-gateway', 8, 73),
    span('inventory', 'checkout', 'inventory.reserve', 'inventory-service', 10, 69),
  ],
  partial: false,
  partial_reasons: [],
  sampling_reasons: ['default_ratio'],
  late_span_count: 0,
  duplicate_span_count: 0,
  conflict_span_count: 0,
};

function renderTraceFlame(node: ReactNode) {
  return render(
    <MemoryRouter>
      <TooltipProvider delayDuration={0}>{node}</TooltipProvider>
    </MemoryRouter>,
  );
}

describe('TraceFlame', () => {
  it('keeps the waterfall responsive and places duration after each bar', () => {
    mocks.useTrace.mockReturnValue({
      data: trace,
      isLoading: false,
      error: null,
    });

    renderTraceFlame(
      <div className="w-[700px]">
        <TraceFlame traceId={trace.trace_id} />
      </div>,
    );

    expect(screen.getByTestId('trace-total-duration').textContent).toContain('184 ms');
    const durations = screen.getAllByTestId('trace-span-duration');
    const bars = screen.getAllByTestId('trace-span-bar');
    expect(durations).toHaveLength(6);
    expect(bars).toHaveLength(6);
    expect(screen.getByText('25 ms')).not.toBeNull();
    for (const [index, duration] of durations.entries()) {
      expect(duration.parentElement?.dataset.testid).toBe('trace-timeline-track');
      expect(bars[index]?.nextElementSibling).toBe(duration);
      expect(duration.className).not.toContain('rounded');
      expect(duration.className).not.toContain('border');
      expect(duration.className).not.toContain('bg-');
    }

    const traceId = screen.getByTestId('trace-id');
    expect(traceId.className).not.toContain('border');
    expect(traceId.className).not.toContain('rounded');

    const viewport = screen.getByTestId('trace-waterfall-viewport');
    expect(viewport.classList.contains('overflow-x-hidden')).toBe(true);
    expect(viewport.querySelector('.min-w-\\[980px\\]')).toBeNull();
    expect(screen.getByRole('link', { name: /关联日志|Related logs/ })).not.toBeNull();
    expect(screen.getByRole('link', { name: /服务指标|Service metrics/ })).not.toBeNull();
  });

  it.each([
    [184_000_000, '184 ms'],
    [25_040_000, '25 ms'],
    [12_340_000, '12.3 ms'],
    [1_234_000, '1.23 ms'],
    [823_000, '823 μs'],
    [12_340, '12.3 μs'],
  ])('formats %i ns as %s', (durationNs, expected) => {
    expect(formatTraceDurationNs(durationNs)).toBe(expected);
  });

  it('uses the same precision rules for millisecond values', () => {
    expect(formatTraceDurationMs(184)).toBe('184 ms');
    expect(formatTraceDurationMs(12.34)).toBe('12.3 ms');
    expect(formatTraceDurationMs(0.823)).toBe('823 μs');
  });

  it('does not show a hover tooltip for waterfall bars', async () => {
    mocks.useTrace.mockReturnValue({
      data: trace,
      isLoading: false,
      error: null,
    });
    const user = userEvent.setup();

    renderTraceFlame(<TraceFlame traceId={trace.trace_id} />);

    const bars = screen.getAllByTestId('trace-span-bar');
    await user.hover(bars[1] as HTMLElement);

    expect(screen.queryByRole('tooltip')).toBeNull();
    expect(screen.getAllByTestId('trace-span-duration')[1]?.textContent).toBe('25 ms');
  });

  it('opens contextual service and span pivots from the waterfall labels', async () => {
    mocks.useTrace.mockReturnValue({
      data: trace,
      isLoading: false,
      error: null,
    });
    const user = userEvent.setup();

    renderTraceFlame(<TraceFlame traceId={trace.trace_id} />);

    await user.click(screen.getByRole('button', { name: 'auth.verify' }));
    const spanLogs = await screen.findByRole('link', { name: /查看 Span 日志|View span logs/ });
    expect(spanLogs.getAttribute('href')).toContain('/logs?');
    expect(spanLogs.getAttribute('href')).toContain('trace_id');
    expect(spanLogs.getAttribute('href')).toContain('span_id');

    await user.keyboard('{Escape}');
    await user.click(screen.getByRole('button', { name: 'auth-service' }));
    expect(
      await screen.findByRole('link', { name: /查看服务指标|View service metrics/ }),
    ).not.toBeNull();
    expect(
      screen.getByRole('link', { name: /查看该服务的日志|View logs for this service/ }),
    ).not.toBeNull();
  });
});
