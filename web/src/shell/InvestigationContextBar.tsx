import { ArrowLeft, Compass, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';

import { useFiltersStore } from '@/stores/useFiltersStore';
import { useInvestigationStack, type Frame } from '@/stores/useInvestigationStack';
import { formatWindowSummary, useTimeStore } from '@/stores/useTimeStore';

/** Param keys, in priority order, that carry a human-recognizable signal. */
const SIGNAL_KEYS = [
  'trace_id',
  'span_id',
  'service',
  'service_name',
  'host',
  'host_id',
  'stream',
  'metric',
  'incident_id',
] as const;

function shorten(value: string): string {
  return value.length > 12 ? `${value.slice(0, 10)}…` : value;
}

/** Pull a short `key:value` signal label out of a frame's params. */
function frameSignal(frame: Frame): string {
  for (const key of SIGNAL_KEYS) {
    const val = frame.params[key];
    if (typeof val === 'string' && val.trim()) {
      return `${key.replace(/_id$|_name$/, '')}:${shorten(val)}`;
    }
  }
  return frame.kind.replace(/_/g, ' ');
}

/**
 * Persistent investigation context strip, rendered between the Topbar and the
 * page body whenever an investigation is active (a non-empty frame stack
 * and/or pinned cross-page filters). It makes Principle #3 (continuity across
 * signals) a *visible* state rather than implicit behavior: the operator
 * always sees how many frames are open, the locked time window, the key
 * signals, and the filters riding along — and can jump back, drop a filter, or
 * clear everything.
 *
 * Publishes its height as `--contextbar-h` so page bodies subtract it from
 * their `calc(100vh …)` height (see `shell/tokens.css` + `PageBody`).
 */
export function InvestigationContextBar() {
  const { t } = useTranslation('shell');
  const nav = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const frames = useInvestigationStack((s) => s.frames);
  const reset = useInvestigationStack((s) => s.reset);
  const timeWindow = useTimeStore((s) => s.window);
  const filters = useFiltersStore((s) => s.filters);
  const removeFilter = useFiltersStore((s) => s.removeFilter);
  const clearFilters = useFiltersStore((s) => s.clearFilters);
  const barRef = React.useRef<HTMLDivElement>(null);

  const sourceTraceId =
    searchParams.get('source') === 'trace' ? searchParams.get('source_id')?.trim() ?? '' : '';
  const active = frames.length > 0 || filters.length > 0 || Boolean(sourceTraceId);

  React.useEffect(() => {
    const root = document.documentElement;
    const bar = barRef.current;
    if (!active || !bar) {
      root.style.setProperty('--contextbar-h', '0px');
      return;
    }

    const publishHeight = () => {
      root.style.setProperty('--contextbar-h', `${Math.round(bar.getBoundingClientRect().height)}px`);
    };
    publishHeight();

    const observer =
      typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(publishHeight);
    observer?.observe(bar);
    return () => {
      observer?.disconnect();
      root.style.setProperty('--contextbar-h', '0px');
    };
  }, [active]);

  if (!active) return null;

  const latest = frames[frames.length - 1];
  const windowLabel = formatWindowSummary(latest?.time_range_override ?? timeWindow);

  return (
    <div
      ref={barRef}
      data-testid="investigation-context-bar"
      className="sticky top-0 z-30 flex h-8 items-center gap-2 border-b border-bd-0 bg-bg-2 pl-3 pr-2 font-sans text-xs text-tx-2"
    >
      {sourceTraceId && (
        <Link
          to={`/traces/${encodeURIComponent(sourceTraceId)}`}
          className="flex shrink-0 items-center gap-1.5 rounded text-tx-2 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
          title={t('context_bar.back_to_trace')}
        >
          <ArrowLeft className="h-3 w-3 text-indigo-soft" />
          <span>{t('context_bar.from_trace', { id: shorten(sourceTraceId) })}</span>
          <span className="font-strong text-indigo-soft">{t('context_bar.back_to_trace')}</span>
        </Link>
      )}

      {frames.length > 0 && (
        <button
          type="button"
          onClick={() => nav('/investigate')}
          className="flex shrink-0 items-center gap-2 rounded hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
          title={t('context_bar.resume')}
        >
          <Compass className="h-3 w-3 shrink-0 text-indigo-soft" />
          <span className="font-strong text-tx-1">{t('context_bar.label')}</span>
          <span className="text-tx-3">·</span>
          <span>
            {frames.length} {t('context_bar.frames')}
          </span>
          <span className="text-tx-3">·</span>
          <span className="font-mono text-tx-2">{windowLabel}</span>
        </button>
      )}

      <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
        {frames.map((frame) => (
          <span
            key={frame.id}
            className="inline-flex shrink-0 items-center rounded border border-indigo/30 bg-indigo-dim px-1.5 py-px font-mono text-xs text-indigo-soft"
          >
            {frameSignal(frame)}
          </span>
        ))}
        {filters.map((filter) => (
          <span
            key={filter.key}
            className="inline-flex shrink-0 items-center gap-1 rounded border border-bd-1 bg-bg-3 py-px pl-1.5 pr-0.5 font-mono text-xs text-tx-1"
          >
            {filter.key}{filter.operator === '!=' ? '≠' : ':'}{shorten(filter.value)}
            <button
              type="button"
              onClick={() => removeFilter(filter.key)}
              aria-label={t('context_bar.remove_filter')}
              title={t('context_bar.remove_filter')}
              className="grid h-3.5 w-3.5 place-items-center rounded text-tx-3 hover:bg-bg-2 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-indigo"
            >
              <X className="h-2.5 w-2.5" />
            </button>
          </span>
        ))}
      </div>

      <button
        type="button"
        onClick={() => {
          reset();
          clearFilters();
          setSearchParams((current) => {
            const next = new URLSearchParams(current);
            next.delete('source');
            next.delete('source_id');
            return next;
          }, { replace: true });
        }}
        className="ml-auto flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-tx-3 hover:bg-bg-3 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo"
      >
        <X className="h-3 w-3" /> {t('context_bar.clear')}
      </button>
    </div>
  );
}
