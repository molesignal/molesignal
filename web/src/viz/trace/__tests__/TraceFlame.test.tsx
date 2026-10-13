import '@/i18n';

import { cleanup, render, screen, within } from '@testing-library/react';
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
      const bar = bars[index];
      expect(duration.parentElement?.dataset.testid).toBe('trace-timeline-track');
      expect(bar?.nextElementSibling).toBe(duration);
      expect(bar?.className).not.toContain('border');
      expect(bar?.className).not.toContain('ring-');
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

  it('colours a bar by how much of the trace its span took, and a failed span red', () => {
    const failed = trace.spans.map((item) =>
      item.span_id === 'inventory' ? { ...item, status: 'ERROR' as const } : item,
    );
    mocks.useTrace.mockReturnValue({
      data: { ...trace, spans: failed },
      isLoading: false,
      error: null,
    });

    renderTraceFlame(<TraceFlame traceId={trace.trace_id} />);

    const tones = Object.fromEntries(
      screen.getAllByTestId('trace-span-bar').map((bar) => [
        bar.getAttribute('aria-label')?.split(',')[0],
        bar.dataset.tone,
      ]),
    );
    // Of 184 ms: all of it, 165, 114, 73 and 25 ms.
    expect(tones).toEqual({
      'POST /api/checkout': 'dominant',
      'checkout.create': 'dominant',
      'payment.authorize': 'major',
      'POST /v2/charges': 'notable',
      'auth.verify': 'minor',
      'inventory.reserve': 'error',
    });

    const fills = screen
      .getAllByTestId('trace-span-bar')
      .map((bar) => bar.className.split(' ').find((name) => name.startsWith('bg-')));
    // Four steps of the scale and the red of the failure: five fills, each said by a class.
    expect(new Set(fills).size).toBe(5);
    expect(fills.filter((fill) => fill === 'bg-red')).toHaveLength(1);

    // The key names the scale once, a swatch to a step.
    const key = screen.getByTestId('trace-share-key');
    expect(key.querySelectorAll('[class*="--share-"]')).toHaveLength(4);
  });

  it('keeps every row of the waterfall the same height', () => {
    mocks.useTrace.mockReturnValue({
      data: trace,
      isLoading: false,
      error: null,
    });

    renderTraceFlame(<TraceFlame traceId={trace.trace_id} />);

    const rows = screen
      .getAllByTestId('trace-span-bar')
      .map((bar) => bar.closest('[data-testid="trace-span-row"]') as HTMLElement);
    expect(rows).toHaveLength(6);
    for (const row of rows) {
      // A fixed height: a name set as an HTTP method and a plain one take the same room.
      expect(row.className).toContain('h-[38px]');
      expect(row.className).not.toContain('min-h-');
    }
  });

  it('selects a span once from the mark of a span that has nothing under it', async () => {
    mocks.useTrace.mockReturnValue({
      data: trace,
      isLoading: false,
      error: null,
    });
    const user = userEvent.setup();
    const onSpanClick = vi.fn();

    renderTraceFlame(<TraceFlame traceId={trace.trace_id} onSpanClick={onSpanClick} />);

    await user.click(screen.getAllByLabelText('Leaf span')[0] as HTMLElement);
    expect(onSpanClick).toHaveBeenCalledTimes(1);
    expect(onSpanClick.mock.calls[0]?.[0]).toMatchObject({ span_id: 'auth' });

    // The mark of a span with spans under it folds them, and selects nothing.
    await user.click(screen.getAllByLabelText('Toggle span children')[0] as HTMLElement);
    expect(onSpanClick).toHaveBeenCalledTimes(1);
    expect(screen.getAllByTestId('trace-span-bar')).toHaveLength(1);
  });

  it('puts no control inside another, and leaves a mark with nothing to fold out of the tab order', () => {
    mocks.useTrace.mockReturnValue({
      data: trace,
      isLoading: false,
      error: null,
    });

    renderTraceFlame(<TraceFlame traceId={trace.trace_id} />);

    const CONTROL = 'button, a[href], [role="button"], [role="link"], [tabindex]';
    const rows = screen.getAllByTestId('trace-span-row');
    expect(rows).toHaveLength(6);
    for (const row of rows) {
      // The row takes the pointer's click but is no control itself.
      expect(row.matches(CONTROL)).toBe(false);
      const controls = [...row.querySelectorAll<HTMLElement>(CONTROL)];
      for (const control of controls) expect(control.querySelector(CONTROL)).toBeNull();

      // One button selects the span, and it comes first: it is what Tab reaches a row by.
      expect(controls[0]).toBe(within(row).getByTestId('trace-span-select'));
      expect(controls[0]?.tagName).toBe('BUTTON');
    }

    // Three of the six spans have spans under them; the mark of the other three is a picture.
    const folds = screen.getAllByLabelText('Toggle span children');
    expect(folds.map((fold) => fold.tagName)).toEqual(['BUTTON', 'BUTTON', 'BUTTON']);
    for (const fold of folds) expect(fold.getAttribute('aria-expanded')).toBe('true');
    const leaves = screen.getAllByLabelText('Leaf span');
    expect(leaves).toHaveLength(3);
    for (const leaf of leaves) {
      expect(leaf.getAttribute('role')).toBe('img');
      expect(leaf.matches(CONTROL)).toBe(false);
    }
  });

  it('selects a span and folds one from the keyboard', async () => {
    mocks.useTrace.mockReturnValue({
      data: trace,
      isLoading: false,
      error: null,
    });
    const user = userEvent.setup();
    const onSpanClick = vi.fn();

    renderTraceFlame(<TraceFlame traceId={trace.trace_id} onSpanClick={onSpanClick} />);

    const row = screen.getAllByTestId('trace-span-row')[2] as HTMLElement;
    const select = within(row).getByRole('button', {
      name: 'Inspect span checkout.create, checkout-service, 165 ms',
    });
    expect(select.getAttribute('aria-pressed')).toBe('false');

    select.focus();
    await user.keyboard('{Enter}');
    expect(onSpanClick).toHaveBeenCalledTimes(1);
    expect(onSpanClick.mock.calls[0]?.[0]).toMatchObject({ span_id: 'checkout' });
    expect(select.getAttribute('aria-pressed')).toBe('true');
    await user.keyboard(' ');
    expect(onSpanClick).toHaveBeenCalledTimes(2);

    // The next stop is the row's fold mark: it folds the spans under the span and selects nothing.
    await user.tab();
    const fold = within(row).getByLabelText('Toggle span children');
    expect(document.activeElement).toBe(fold);
    await user.keyboard('{Enter}');
    expect(fold.getAttribute('aria-expanded')).toBe('false');
    expect(screen.getAllByTestId('trace-span-bar')).toHaveLength(3);
    await user.keyboard(' ');
    expect(fold.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getAllByTestId('trace-span-bar')).toHaveLength(6);
    expect(onSpanClick).toHaveBeenCalledTimes(2);
  });

  it('tells the button that selects a span what the row says of it', () => {
    const failed = trace.spans.map((item) =>
      item.span_id === 'inventory' ? { ...item, status: 'ERROR' as const } : item,
    );
    mocks.useTrace.mockReturnValue({
      data: { ...trace, spans: failed },
      isLoading: false,
      error: null,
    });

    renderTraceFlame(<TraceFlame traceId={trace.trace_id} />);

    const select = screen.getByRole('button', {
      name: 'Inspect span inventory.reserve, inventory-service, 69 ms',
    });
    expect(select).toHaveAccessibleDescription('error');
    expect(
      screen.getByRole('button', { name: 'Inspect span auth.verify, auth-service, 25 ms' }),
    ).not.toHaveAttribute('aria-describedby');
  });

  it('says a trace cannot be displayed when the response carries no spans', () => {
    mocks.useTrace.mockReturnValue({
      // What a proxy or an older backend may answer with: a body that is not a trace.
      data: {} as TraceResponse,
      isLoading: false,
      error: null,
    });

    renderTraceFlame(<TraceFlame traceId={trace.trace_id} />);

    expect(screen.getByText(/Trace can't be displayed|无法显示该链路/)).not.toBeNull();
    expect(screen.queryAllByTestId('trace-span-bar')).toHaveLength(0);
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
