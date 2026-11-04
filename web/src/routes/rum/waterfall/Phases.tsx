import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import type { Phases } from './model';
import { formatDurationMs } from '../_helpers';

/** The parts of a request in the order they happen, each with the fill it is drawn in. */
export const PHASES: ReadonlyArray<{ key: keyof Phases; fill: string }> = [
  { key: 'redirect', fill: 'var(--tx-4)' },
  { key: 'dns', fill: 'var(--chart-1)' },
  { key: 'connect', fill: 'var(--chart-2)' },
  { key: 'tls', fill: 'var(--chart-7)' },
  { key: 'first_byte', fill: 'var(--chart-8)' },
  { key: 'download', fill: 'var(--chart-6)' },
];

/** The parts a request took any time on, and how long each took. */
export function timedPhases(phases: Phases): Array<{ key: keyof Phases; fill: string; ms: number }> {
  return PHASES.flatMap((phase) => {
    const ms = phases[phase.key];

    return ms !== null && ms > 0 ? [{ ...phase, ms }] : [];
  });
}

/** The part that took the longest, when any part took any time. */
export function longestPhase(phases: Phases): keyof Phases | null {
  const parts = timedPhases(phases);

  return parts.reduce<{ key: keyof Phases; ms: number } | null>((top, part) => (top === null || part.ms > top.ms ? part : top), null)?.key ?? null;
}

/**
 * What a request took its time on: a bar as long as the request, cut into its parts, and under it each
 * part with its time. The part that took the longest is the one to look at, and says so.
 */
export function PhaseBar({ phases, title, className }: { phases: Phases; title?: string; className?: string }) {
  const { t } = useTranslation('rum');
  const parts = timedPhases(phases);
  const total = parts.reduce((sum, part) => sum + part.ms, 0);
  const top = longestPhase(phases);

  if (parts.length === 0) return null;

  return (
    <div data-rum-phases="" className={cn('flex flex-col gap-2', className)}>
      {title && <div className="font-sans text-xs font-strong text-tx-3">{title}</div>}
      <div role="img" aria-label={t('waterfall.phases.aria', { total: formatDurationMs(total) })} className="flex h-3.5 gap-0.5">
        {parts.map((part) => (
          <span
            key={part.key}
            data-rum-phase={part.key}
            className="min-w-[3px] first:rounded-l-[4px] last:rounded-r-[4px]"
            style={{ flex: `${part.ms} 1 0`, background: part.fill, opacity: 0.9 }}
          />
        ))}
      </div>
      <dl className="m-0 grid grid-cols-2 gap-x-3 gap-y-1 font-sans text-xs">
        {parts.map((part) => (
          <div key={part.key} className="flex min-w-0 items-center gap-1.5">
            <span aria-hidden className="h-2 w-2 shrink-0 rounded-[2px]" style={{ background: part.fill }} />
            <dt className={cn('min-w-0 truncate', part.key === top ? 'font-strong text-[var(--orange-ink)]' : 'text-tx-2')}>
              {t(`waterfall.phases.${part.key}`)}
            </dt>
            <dd className="m-0 ml-auto font-strong tabular-nums text-tx-0">{formatDurationMs(part.ms)}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}
