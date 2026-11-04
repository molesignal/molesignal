import { cn } from '@/shell/lib/cn';
import { severityBarClass } from '@/viz/SeverityRail';

import { logLevelTone, type LogLevelTone } from './viewModel';

/**
 * How a level is written. A failure and a warning take their colour; the
 * ordinary run of things is plain text, so a screen of INFO is not a screen
 * of colour, and what went wrong is what stands out.
 */
const LEVEL_TEXT: Record<LogLevelTone, string> = {
  error: 'text-[var(--red-ink)]',
  warn: 'text-[var(--yellow-ink)]',
  info: 'text-tx-1',
  quiet: 'text-tx-3',
};

/** The colour grammar of severity, shared with alerts and stream health. */
const LEVEL_SEVERITY: Record<LogLevelTone, string> = {
  error: 'error',
  warn: 'warn',
  info: 'info',
  quiet: 'debug',
};

/** The edge a log row carries down its left side: loud for a failure or a warning, faint otherwise. */
export const LEVEL_EDGE: Record<LogLevelTone, string> = {
  error: 'border-l-red',
  warn: 'border-l-yellow',
  info: 'border-l-blue/30',
  quiet: 'border-l-tx-0/10',
};

/** The bar a log row carries down its left edge, as a trace row carries how it ended: loud for a failure or a warning, faint otherwise. */
export const LEVEL_BAR: Record<LogLevelTone, string> = {
  error: 'bg-red',
  warn: 'bg-[var(--yellow)]',
  info: 'bg-[var(--blue)] opacity-30',
  quiet: 'bg-tx-0/10',
};

/**
 * A level in a table cell, written as the trace table writes an HTTP method:
 * the word alone in its colour, in the face and size of the rest of the row,
 * with no chip behind it.
 */
export function LevelChip({ level }: { level: string }) {
  return <LevelWord level={level} size="text-xs" mono={false} />;
}

/**
 * A level as a word in its colour, at the size of the table it stands in unless told another. A
 * failure and a warning are in the strong weight, the ordinary run of things in the body one. It
 * is in the monospace face, except in a list table (`mono={false}`).
 */
export function LevelWord({ level, size = 'type-micro', mono = true }: { level: string; size?: string; mono?: boolean }) {
  const tone = logLevelTone(level);
  return (
    <span className={cn(size, mono && 'font-mono', tone === 'error' || tone === 'warn' ? 'font-strong' : 'font-body', LEVEL_TEXT[tone])}>{level}</span>
  );
}

/** A level as a mark and a word: the head of a record that is read on its own. */
export function LevelMark({ level, className }: { level: string; className?: string }) {
  const tone = logLevelTone(level);

  return (
    <span className={cn('inline-flex items-center gap-[6px]', className)}>
      <span
        aria-hidden="true"
        className={cn('h-[8px] w-[8px] shrink-0 rounded-full', severityBarClass(LEVEL_SEVERITY[tone]))}
      />
      <LevelWord level={level} size="text-xs" />
    </span>
  );
}
