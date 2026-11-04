import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { writeClipboardText } from '@/lib/clipboard';
import { useTimeFormatter, zonedMoment } from '@/lib/time';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';

import { formatTraceDurationNs } from '../duration';
import { HTTP_STATUS_TEXT, httpStatusOf, httpStatusTone, OUTCOME_FILL, requestOutcome } from '../model/span';
import type { TraceTree } from '../model/tree';
import { TraceOperationName } from '../TraceOperationName';

/** A way out of the trace into another signal, with where it goes. */
export interface TraceJump {
  key: 'logs' | 'metrics' | 'profiles' | 'session';
  to: string;
}

const LINK = 'whitespace-nowrap rounded font-sans text-xs text-[var(--blue-ink)] underline-offset-2 hover:underline focus-visible:underline focus-visible:outline-none';

/**
 * The head of a trace, fixed over its views: the request it began with and
 * how it ended, how long it took and when, how big it is, its id, and the
 * ways into the logs, metrics, profiles and session around it.
 */
export function TraceSummary({
  tree,
  jumps,
  tools,
  actions,
  outgoingLinks,
}: {
  tree: TraceTree;
  jumps: readonly TraceJump[];
  /** What is done with the trace wherever it is shown: comparing it, downloading it. */
  tools?: React.ReactNode;
  /** The place's own controls: the neighbours in the list, the trace's page, closing. */
  actions?: React.ReactNode;
  /** How many links the trace's spans keep to other spans. */
  outgoingLinks: number;
}) {
  const { t, i18n } = useTranslation('traces');
  const fmt = useTimeFormatter();
  const { root } = tree;
  const status = httpStatusOf(root.span);
  const startMs = Math.floor(tree.startNs / 1_000_000);
  const start = zonedMoment(startMs, fmt.tz);
  const ago = relativeTime(startMs, i18n.resolvedLanguage ?? i18n.language);

  return (
    <header data-testid="trace-summary" className="shrink-0 border-b border-bd-0 px-3 pb-2 pt-2">
      <div className="flex min-h-8 items-center gap-2">
        <span
          aria-hidden="true"
          className={cn('h-2.5 w-2.5 shrink-0 rounded-full', OUTCOME_FILL[requestOutcome(tree.errorCount > 0, status)])}
        />
        <h2 className="flex min-w-0 flex-1 items-center gap-1.5 font-sans text-sm">
          <span className="min-w-0 max-w-[40%] shrink-[2] truncate font-strong text-tx-1">{root.service}</span>
          <span aria-hidden="true" className="shrink-0 text-tx-4">›</span>
          <TraceOperationName operation={root.span.operation} className="min-w-0 font-strong text-tx-0" />
          {status !== null && (
            <span className={cn('shrink-0 font-sans text-xs font-strong leading-5 tabular-nums', HTTP_STATUS_TEXT[httpStatusTone(status)])}>
              {status}
            </span>
          )}
          {tree.errorCount > 0 && (
            <span className="shrink-0 rounded-sm bg-red-dim px-1.5 font-sans text-xs font-strong leading-5 text-[var(--red-ink)]">
              {t('view.summary.errors', { count: tree.errorCount })}
            </span>
          )}
        </h2>
        {(tools || actions) && (
          <div className="flex shrink-0 items-center gap-1">
            {tools}
            {actions}
          </div>
        )}
      </div>

      <div className="mt-1 flex flex-wrap items-center justify-between gap-x-4 gap-y-1">
        <dl className="flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-1 font-sans text-xs text-tx-2" data-testid="trace-summary-facts">
          <Fact label={t('view.summary.duration')}>
            <span className="font-strong tabular-nums text-tx-0" data-testid="trace-total-duration">{formatTraceDurationNs(tree.totalNs)}</span>
          </Fact>
          <Fact label={t('view.summary.start')}>
            <span className="tabular-nums">{start.isValid() ? start.format('YYYY-MM-DD HH:mm:ss.SSS') : '—'}</span>
            {ago && <span className="ml-1 text-tx-3">({ago})</span>}
          </Fact>
          <Fact label={t('view.summary.size')}>
            {t('view.summary.spans', { count: tree.nodes.length })} · {t('view.summary.services', { count: tree.services.length })} · {t('view.summary.depth', { depth: tree.maxDepth + 1 })}
          </Fact>
          <Fact label={t('view.summary.trace_id')}>
            <span className="inline-flex min-w-0 items-center gap-0.5">
              <span className="max-w-[260px] truncate tabular-nums text-tx-1" data-testid="trace-id" title={tree.traceId}>{tree.traceId}</span>
              <CopyIconButton
                className="h-6 w-6 shrink-0 text-tx-3 hover:text-tx-0"
                label={t('detail.copy_trace_id')}
                onClick={() => {
                  void writeClipboardText(tree.traceId);
                }}
              />
            </span>
          </Fact>
          {tree.orphanCount > 0 && (
            <span className="rounded-sm bg-[var(--yellow-tint)] px-1.5 leading-5 text-[var(--yellow-ink)]" title={t('view.summary.orphans_hint')}>
              {t('view.summary.orphans', { count: tree.orphanCount })}
            </span>
          )}
          {tree.trace.truncated && (
            <span className="rounded-sm bg-[var(--yellow-tint)] px-1.5 leading-5 text-[var(--yellow-ink)]">{t('detail.truncated')}</span>
          )}
          {outgoingLinks > 0 && <span className="text-tx-2">{t('view.summary.links', { count: outgoingLinks })}</span>}
        </dl>
        {jumps.length > 0 && (
          <nav aria-label={t('view.summary.jumps')} className="flex shrink-0 items-center gap-3">
            {jumps.map((jump) => (
              <Link key={jump.key} to={jump.to} className={LINK} data-testid={`trace-jump-${jump.key}`}>
                {t(`view.summary.jump.${jump.key}`)}
              </Link>
            ))}
          </nav>
        )}
      </div>
    </header>
  );
}

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex min-w-0 items-center gap-1">
      <dt className="sr-only">{label}</dt>
      <dd className="flex min-w-0 items-center">{children}</dd>
    </div>
  );
}

const STEPS: Array<[Intl.RelativeTimeFormatUnit, number]> = [
  ['second', 60],
  ['minute', 60],
  ['hour', 24],
  ['day', 30],
  ['month', 12],
  ['year', Number.POSITIVE_INFINITY],
];

/** How long ago a moment was, in the reader's language: "3 minutes ago". */
export function relativeTime(ms: number, language: string, now = Date.now()): string | null {
  if (!Number.isFinite(ms) || ms <= 0) return null;

  let value = (ms - now) / 1_000;
  const format = new Intl.RelativeTimeFormat(language, { numeric: 'auto' });

  for (const [unit, size] of STEPS) {
    if (Math.abs(value) < size) return format.format(Math.round(value), unit);
    value /= size;
  }

  return null;
}
