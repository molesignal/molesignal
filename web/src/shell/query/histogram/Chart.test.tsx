import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { DEFAULT_USER_PREFERENCES } from '@/api/me';

import { ResultHistogram } from './Chart';
import type { HistogramCountsAnswer, HistogramCountsSource } from './counts';
import type { HistogramPoint } from './model';

const micros = (iso: string) => Date.parse(iso) * 1_000;
/** Twenty-odd seconds of a busy minute: clumps, a quiet stretch, one bar that holds an error. */
const POINTS: HistogramPoint[] = [
  ['23:22:47.100', false],
  ['23:22:47.600', false],
  ['23:22:55.050', false],
  ['23:22:55.300', true],
  ['23:22:55.450', false],
  ['23:23:08.400', false],
  ['23:23:20.800', false],
].map(([time, error]) => ({ micros: micros(`2026-10-11T${time}Z`), error: error as boolean }));
const LABELS = { chart: 'Histogram', ok: 'Non-error', errors: 'Errors' };
const DAY = { start: micros('2026-10-11T00:00:00Z'), end: micros('2026-10-12T00:00:00Z') };

afterEach(() => cleanup());

function renderHistogram(
  props: Partial<React.ComponentProps<typeof ResultHistogram>> = {},
  preferences: Partial<typeof DEFAULT_USER_PREFERENCES> = {},
) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: Infinity } } });
  // The reader's clock is UTC, whatever zone the tests run in.
  client.setQueryData(['me', 'preferences'], { ...DEFAULT_USER_PREFERENCES, timezone: 'UTC', ...preferences });
  const onRangeSelect = vi.fn();
  const chart = (more: Partial<React.ComponentProps<typeof ResultHistogram>> = {}) => (
    <QueryClientProvider client={client}>
      <ResultHistogram points={POINTS} range={DAY} labels={LABELS} onRangeSelect={onRangeSelect} {...props} {...more} />
    </QueryClientProvider>
  );

  const view = render(chart());
  const module = view.container.querySelector<HTMLElement>('[data-explorer-module="histogram"]');
  const plot = module?.querySelector<HTMLElement>('[data-histogram-plot]') ?? null;

  // jsdom lays nothing out: the plot is given the box the pointer is measured against.
  if (plot) {
    vi.spyOn(plot, 'getBoundingClientRect').mockReturnValue({
      x: 100, y: 200, left: 100, top: 200, right: 900, bottom: 264, width: 800, height: 64, toJSON: () => ({}),
    });
  }

  return {
    module,
    plot,
    onRangeSelect,
    /** The chart as it stands now: it comes and goes with what there is to draw. */
    current: () => view.container.querySelector<HTMLElement>('[data-explorer-module="histogram"]'),
    rerender: (more: Partial<React.ComponentProps<typeof ResultHistogram>>) => view.rerender(chart(more)),
  };
}

/** A source of counts that answers `answer`, or never answers when there is none. */
function countsOf(answer?: HistogramCountsAnswer | Error, key: string = 'query'): HistogramCountsSource {
  return {
    key: [key],
    load: vi.fn(() =>
      answer === undefined
        ? new Promise<HistogramCountsAnswer>(() => undefined)
        : answer instanceof Error
          ? Promise.reject(answer)
          : Promise.resolve(answer),
    ),
  };
}

const QUARTER_HOUR = 15 * 60 * 1_000_000;
/** A day of matches: a busy quarter of an hour in the morning with errors, a quiet one at night. */
const COUNTED: HistogramCountsAnswer = {
  bars: [
    { startMicros: micros('2026-10-11T10:00:00Z'), total: 1200, errors: 30 },
    { startMicros: micros('2026-10-11T23:15:00Z'), total: 5, errors: 0 },
  ],
  split: true,
};

const texts = (root: Element | null, selector: string) =>
  Array.from(root?.querySelectorAll(selector) ?? []).map((node) => node.textContent);

describe('ResultHistogram', () => {
  it('is drawn as a histogram is read: counts up the side, time along the bottom, bars of one named length', () => {
    const { module } = renderHistogram();
    const frame = module?.querySelector<HTMLElement>('[data-histogram-frame]');

    // A page of results inside a day: the bars span the results, in half-second bars.
    expect(frame?.dataset.barMicros).toBe('500000');
    expect(Number(frame?.dataset.frameStart)).toBe(micros('2026-10-11T23:22:45.500Z'));
    expect(Number(frame?.dataset.frameEnd)).toBe(micros('2026-10-11T23:23:22.500Z'));

    // Whole counts, to a line the tallest bar (three results) does not pass.
    expect(texts(module, '[data-histogram-axis="count"] span')).toEqual(['0', '1', '2', '3']);
    // Marks on the edges of bars, at a step a reader counts in.
    expect(texts(module, '[data-histogram-axis="time"] .type-micro')).toEqual([
      '23:22:50', '23:22:55', '23:23:00', '23:23:05', '23:23:10', '23:23:15', '23:23:20',
    ]);
    // What the axis leaves unsaid.
    expect(frame?.querySelector('time')?.textContent).toBe('2026-10-11');
    expect(frame?.textContent).toContain('500 ms per bar');

    // A bar for every stretch that holds something, and none for the empty ones.
    expect(texts(module, '[data-histogram-bar]')).toHaveLength(5);
    expect(
      Array.from(module?.querySelectorAll<HTMLElement>('[data-histogram-bar]') ?? []).map((bar) => bar.dataset.histogramBar),
    ).toEqual(['1', '1', '3', '1', '1']);
  });

  it('counts in fewer steps when it is given less room', () => {
    const { module } = renderHistogram({ short: true });

    expect(texts(module, '[data-histogram-axis="count"] span')).toEqual(['0', '1', '2', '3']);
    expect(module?.querySelector('[data-histogram-plot]')?.className).toContain('h-[64px]');
  });

  it('writes the time as the reader has asked to see it', () => {
    const { module } = renderHistogram({}, { time_format: 'local_12h', date_format: 'dd_mm_yyyy_slash', timezone: 'Asia/Shanghai' });

    // The same instants, eight hours on: the next morning there.
    expect(texts(module, '[data-histogram-axis="time"] .type-micro')[0]).toBe('07:22:50 AM');
    expect(module?.querySelector('[data-histogram-frame] time')?.textContent).toBe('12/10/2026');
  });

  it('follows the zone of the view it heads, where that is not the reader\'s own', () => {
    const { module } = renderHistogram({ timezone: 'Asia/Shanghai' });

    // The table under it shows these traces at 07:22 the next morning; so does the axis.
    expect(texts(module, '[data-histogram-axis="time"] .type-micro')[0]).toBe('07:22:50');
    expect(module?.querySelector('[data-histogram-frame] time')?.textContent).toBe('2026-10-12');
  });

  it('draws nothing when no result carries a time', () => {
    const { module } = renderHistogram({ points: [{ micros: null, error: false }] });

    expect(module).toBeNull();
  });

  it('names itself and what it shows to assistive technology', () => {
    renderHistogram();

    expect(screen.getByRole('group').getAttribute('aria-label')).toBe(
      'Histogram. The 7 results that are loaded, between 2026-10-11 23:22:45.500 and 2026-10-11 23:23:22.500, ' +
        '500 ms per bar, at most 3 in one bar. ' +
        'Left and right arrows move from bar to bar; Enter narrows the time range to the bar.',
    );
  });

  it('says under the chart that the bars are of the loaded results', () => {
    const { module } = renderHistogram();

    expect(module?.dataset.histogramCounting).toBe('loaded');
    expect(module?.querySelector('[data-histogram-scope]')?.textContent).toBe('The 7 results that are loaded');
    expect(renderHistogram({ points: POINTS.slice(0, 1) }).module?.querySelector('[data-histogram-scope]')?.textContent).toBe(
      'The one result that is loaded',
    );
  });

  it('reads out the bar under the pointer above the plot, and lays nothing on the bars', async () => {
    const user = userEvent.setup();
    const { module, plot } = renderHistogram();

    // The bar from 23:22:55.000: nineteen bars in, of seventy-four across 800 px.
    await user.pointer({ target: plot!, coords: { clientX: 100 + (19.5 / 74) * 800, clientY: 230 } });

    const readout = screen.getByRole('tooltip');
    expect(readout.textContent).toContain('23:22:55.000 – 23:22:55.500');
    expect(within(readout).getByText('Non-error').nextElementSibling?.textContent).toBe('2');
    expect(within(readout).getByText('Errors').nextElementSibling?.textContent).toBe('1');
    // It hangs from the window, not from the module: nothing of it is inside the plot.
    expect(module?.contains(readout)).toBe(false);
    expect(module?.querySelector('button')).toBeNull();
    expect(module?.querySelector('[data-histogram-cursor]')).not.toBeNull();

    await user.unhover(plot!);
    expect(screen.queryByRole('tooltip')).toBeNull();
  });

  it('narrows the time range to the bars a drag crosses, edge to edge', async () => {
    const user = userEvent.setup();
    const { module, plot, onRangeSelect } = renderHistogram();
    const x = (bar: number) => 100 + ((bar + 0.5) / 74) * 800;

    await user.pointer([
      { keys: '[MouseLeft>]', target: plot!, coords: { clientX: x(30), clientY: 230 } },
      { target: plot!, coords: { clientX: x(18), clientY: 230 } },
    ]);
    expect(module?.querySelector('[data-histogram-selection]')).not.toBeNull();
    expect(screen.queryByRole('tooltip')).toBeNull();

    await user.pointer({ keys: '[/MouseLeft]', target: plot!, coords: { clientX: x(18), clientY: 230 } });

    // From the start of the first bar crossed to the end of the last, whichever way it was dragged.
    expect(onRangeSelect).toHaveBeenCalledExactlyOnceWith({
      from: micros('2026-10-11T23:22:54.500Z'),
      to: micros('2026-10-11T23:23:01.000Z'),
    });
    expect(module?.querySelector('[data-histogram-selection]')).toBeNull();
  });

  it('selects nothing on a click', async () => {
    const user = userEvent.setup();
    const { plot, onRangeSelect } = renderHistogram();

    await user.pointer({ keys: '[MouseLeft]', target: plot!, coords: { clientX: 400, clientY: 230 } });

    expect(onRangeSelect).not.toHaveBeenCalled();
  });

  it('can be read and narrowed from the keyboard', async () => {
    const user = userEvent.setup();
    const { module, plot, onRangeSelect } = renderHistogram();

    plot!.focus();
    // The first arrow lands on the newest bar; the arrows walk from there.
    await user.keyboard('{ArrowLeft}');
    expect(screen.getByRole('tooltip').textContent).toContain('23:23:22.000 – 23:23:22.500');
    await user.keyboard('{ArrowLeft}{ArrowLeft}{ArrowLeft}');
    expect(screen.getByRole('tooltip').textContent).toContain('23:23:20.500 – 23:23:21.000');
    expect(module?.querySelector('[aria-live="polite"]')?.textContent).toBe(
      '23:23:20.500 to 23:23:21.000: 1 in all, 0 of them errors',
    );

    await user.keyboard('{Enter}');
    expect(onRangeSelect).toHaveBeenCalledExactlyOnceWith({
      from: micros('2026-10-11T23:23:20.500Z'),
      to: micros('2026-10-11T23:23:21.000Z'),
    });

    await user.keyboard('{Home}');
    expect(screen.getByRole('tooltip').textContent).toContain('23:22:45.500 – 23:22:46.000');
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('tooltip')).toBeNull();
  });
});

describe('ResultHistogram with counts of everything the query matches', () => {
  it('asks for the bars it has settled on, and draws every match across the queried range', async () => {
    const counts = countsOf(COUNTED);
    const { current } = renderHistogram({ counts });

    await waitFor(() => expect(current()?.dataset.histogramCounting).toBe('all'));

    // A day in ninety-six bars: a quarter of an hour each, from midnight on the reader's clock.
    expect(counts.load).toHaveBeenCalledExactlyOnceWith(
      { range: DAY, binMicros: QUARTER_HOUR, originMicros: DAY.start },
      expect.any(AbortSignal),
    );
    const frame = current()?.querySelector<HTMLElement>('[data-histogram-frame]');
    expect(frame?.dataset.barMicros).toBe(String(QUARTER_HOUR));
    expect(Number(frame?.dataset.frameStart)).toBe(DAY.start);
    expect(Number(frame?.dataset.frameEnd)).toBe(DAY.end);

    // The counts, not the seven loaded results: two bars, on a scale that holds the taller.
    expect(
      Array.from(current()?.querySelectorAll<HTMLElement>('[data-histogram-bar]') ?? []).map((bar) => bar.dataset.histogramBar),
    ).toEqual(['1200', '5']);
    expect(texts(current(), '[data-histogram-axis="count"] span')).toEqual(['0', '500', '1K', '1.5K']);
    expect(texts(current(), '[data-histogram-axis="time"] .type-micro').slice(0, 3)).toEqual(['10-11', '03:00', '06:00']);
    expect(current()?.querySelector('[data-histogram-scope]')?.textContent).toBe('All 1,205 results that match');
    expect(frame?.textContent).toContain('15 min per bar');
    expect(screen.getByRole('group').getAttribute('aria-label')).toContain('Histogram. All 1,205 results that match, between');
  });

  it('stands as an empty frame of the queried range while the counts are on their way', async () => {
    const { current } = renderHistogram({ counts: countsOf() });

    await waitFor(() => expect(current()?.querySelector('[data-histogram-waiting]')).not.toBeNull());

    expect(current()?.dataset.histogramCounting).toBe('counting');
    expect(current()?.getAttribute('aria-busy')).toBe('true');
    expect(current()?.querySelectorAll('[data-histogram-bar]')).toHaveLength(0);
    expect(Number(current()?.querySelector<HTMLElement>('[data-histogram-frame]')?.dataset.frameStart)).toBe(DAY.start);
    // A line to stand on, and no scale to read against yet.
    expect(texts(current(), '[data-histogram-axis="count"] span')).toEqual(['0']);
    expect(current()?.querySelector('[data-histogram-scope]')?.textContent).toBe('Counting all results that match…');
  });

  it('draws the loaded results, and says why, when everything cannot be counted', async () => {
    const { current } = renderHistogram({ counts: countsOf({ unavailable: 'time_budget' }) });

    await waitFor(() => expect(current()?.dataset.histogramCounting).toBe('loaded'));

    // The page of results, framed by its own span as it is without counts.
    expect(current()?.querySelector<HTMLElement>('[data-histogram-frame]')?.dataset.barMicros).toBe('500000');
    expect(current()?.querySelectorAll('[data-histogram-bar]')).toHaveLength(5);
    expect(current()?.querySelector('[data-histogram-scope]')?.textContent).toBe(
      'The 7 results that are loaded: counting all that match took too long',
    );
    expect(current()?.getAttribute('aria-busy')).toBeNull();
  });

  it('draws the loaded results when the counts do not come back', async () => {
    const { current } = renderHistogram({ counts: countsOf(new Error('network')) });

    await waitFor(() =>
      expect(current()?.querySelector('[data-histogram-scope]')?.textContent).toBe(
        'The 7 results that are loaded: all that match could not be counted',
      ),
    );
  });

  it('draws whole bars, and reads out no division, where errors are not counted apart', async () => {
    const user = userEvent.setup();
    const { current } = renderHistogram({ counts: countsOf({ ...COUNTED, split: false }) });

    await waitFor(() => expect(current()?.dataset.histogramCounting).toBe('all'));
    const plot = current()!.querySelector<HTMLElement>('[data-histogram-plot]')!;
    vi.spyOn(plot, 'getBoundingClientRect').mockReturnValue({
      x: 0, y: 200, left: 0, top: 200, right: 960, bottom: 264, width: 960, height: 64, toJSON: () => ({}),
    });

    // The bar from 10:00 is the forty-first of ninety-six.
    expect(current()?.querySelector('[data-histogram-bar="1200"]')?.children).toHaveLength(1);
    await user.pointer({ target: plot, coords: { clientX: 40.5 * 10, clientY: 230 } });

    const readout = screen.getByRole('tooltip');
    expect(readout.textContent).toContain('10:00 – 10:15');
    // The axis says 1K and 1.5K; the readout says the count itself.
    expect(readout.textContent).toContain('1,200');
    expect(within(readout).queryByText('Errors')).toBeNull();
  });

  it('keeps the bars it had, set back, while another query is counted', async () => {
    const { current, rerender } = renderHistogram({ counts: countsOf(COUNTED) });
    await waitFor(() => expect(current()?.dataset.histogramCounting).toBe('all'));

    rerender({ counts: countsOf(undefined, 'another query') });

    await waitFor(() => expect(current()?.dataset.histogramCounting).toBe('counting'));
    expect(current()?.querySelectorAll('[data-histogram-bar]')).toHaveLength(2);
    expect(current()?.querySelector('[data-histogram-bar]')?.className).toContain('opacity-40');
    expect(current()?.querySelector('[data-histogram-waiting]')).toBeNull();
    expect(current()?.querySelector('[data-histogram-scope]')?.textContent).toBe('Counting all results that match…');
  });

  it('counts the same query once, however often it is drawn', async () => {
    const counts = countsOf(COUNTED);
    const { current, rerender } = renderHistogram({ counts });
    await waitFor(() => expect(current()?.dataset.histogramCounting).toBe('all'));

    // Another page of the same query: other loaded results, the same key.
    rerender({ points: POINTS.slice(0, 2), counts: { ...counts } });

    expect(current()?.dataset.histogramCounting).toBe('all');
    expect(counts.load).toHaveBeenCalledTimes(1);
  });

  it('asks for nothing without a range to count in', () => {
    const counts = countsOf(COUNTED);
    const { module } = renderHistogram({ counts, range: null });

    expect(module?.dataset.histogramCounting).toBe('loaded');
    expect(counts.load).not.toHaveBeenCalled();
  });
});
