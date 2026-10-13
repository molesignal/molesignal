import { ChevronRight, ListFilter, Loader2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { useTimeFormatter } from '@/lib/time';
import { ChromeButton, uiTableHeaderClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shell/ui/tooltip';

import { LEVEL_EDGE, LevelWord } from '../LevelMark';
import {
  formatLogMoment,
  logLevelLabel,
  logLevelTone,
  logSourceLabel,
  logStatement,
  momentsShareDay,
  type LogClock,
  type LogEntry,
  type LogLevelTone,
} from '../viewModel';
import { logPatterns, type LogPattern, type PatternToken } from './model';

/** The logs of a pattern shown when it is opened, and how many more each further ask shows. */
const EXAMPLES = 6;
const EXAMPLES_STEP = 50;
/** How many levels a row names before it counts the rest. */
const LEVELS_NAMED = 3;
const LEVEL_RANK: Record<LogLevelTone, number> = { error: 3, warn: 2, info: 1, quiet: 0 };

/**
 * The columns, as widths the head and every row share. The pattern takes the
 * room that is left; as the view narrows the source is given up, then the
 * share, and the levels are cut to the loudest.
 */
const CELL = {
  count: 'w-[52px] shrink-0 text-right',
  share: 'hidden w-[84px] shrink-0 [@container_(min-width:600px)]:block',
  pattern: 'min-w-0 flex-1',
  level: 'w-[60px] shrink-0 [@container_(min-width:600px)]:w-[176px]',
  source: 'hidden w-[152px] shrink-0 [@container_(min-width:800px)]:block',
  action: 'w-7 shrink-0',
} as const;
const ROW = 'flex items-center gap-3 border-b border-l-[3px] border-bd-0 pl-[13px] pr-4';

/** How the view came by its logs, where it read them itself: further back than a page of the list. */
export interface PatternSampleState {
  /** The rows are what was read, not yet only the page the list had. */
  held: boolean;
  reading: boolean;
  /** Older logs match and have not been read. */
  more: boolean;
  failed: boolean;
  /** How many logs one further ask reads. */
  step: number;
  onReadMore: () => void;
  onRetry: () => void;
}

interface LogPatternsViewProps {
  rows: LogEntry[];
  /** The log open in the drawer, by its place among the rows. */
  selectedIndex?: number | null | undefined;
  /** Absent where the rows are all there is: what a statement the reader wrote returned. */
  sample?: PatternSampleState | undefined;
  /** Opens one log, by its place among the rows. */
  onOpenLog: (index: number) => void;
  /** Narrows the query to the logs that have these words in this field. Absent where the query cannot say so. */
  onFilter?: ((field: string, fragment: string) => void) | undefined;
}

/**
 * The logs by what they say, instead of one by one: each kind of message once,
 * with how many logs say it, how they divide by level, and where they come
 * from. A place where every log says something of its own (a number, an
 * identifier, an address) is drawn as an open slot. Opening a pattern shows
 * its logs; asking for it puts it in the query. Under the patterns stands what
 * they were read from.
 */
export function LogPatternsView({ rows, selectedIndex, sample, onOpenLog, onFilter }: LogPatternsViewProps) {
  const { t } = useTranslation('logs');
  const time = useTimeFormatter();
  const clock = React.useMemo<LogClock>(
    () => ({ tz: time.tz, format: time.format, dateFormat: time.dateFormat }),
    [time.dateFormat, time.format, time.tz],
  );
  const { patterns, unread } = React.useMemo(() => logPatterns(rows), [rows]);
  // Within a day the day is not said on every log.
  const scope = React.useMemo(
    () => (momentsShareDay(rows.map((row) => row.micros), clock.tz) ? 'time' : 'day'),
    [clock.tz, rows],
  );
  const [open, setOpen] = React.useState<string | null>(null);

  return (
    <TooltipProvider delayDuration={250}>
      <div data-log-patterns className="flex h-full min-h-0 flex-col [container-type:inline-size]">
        <div className="min-h-0 flex-1 overflow-auto">
          <div className={cn('sticky top-0 z-10 border-l-transparent bg-bg-1 py-2', ROW, uiTableHeaderClass)}>
            <span className={CELL.count}>{t('explore.patterns.events')}</span>
            <span className={CELL.share}>{t('explore.patterns.share')}</span>
            <span className={CELL.pattern}>{t('explore.patterns.pattern', { count: patterns.length })}</span>
            <span className={CELL.level}>{t('explore.patterns.level')}</span>
            <span className={CELL.source}>{t('explore.patterns.source')}</span>
            <span className={CELL.action} />
          </div>
          {patterns.map((pattern) => (
            <PatternRow
              key={pattern.key}
              pattern={pattern}
              rows={rows}
              clock={clock}
              scope={scope}
              selectedIndex={selectedIndex ?? null}
              open={open === pattern.key}
              onOpenChange={(next) => setOpen(next ? pattern.key : null)}
              onOpenLog={onOpenLog}
              onFilter={onFilter}
            />
          ))}
          {unread > 0 && (
            <p data-log-patterns-unread className="px-4 py-3 font-sans text-xs text-tx-2">
              {t('explore.patterns.unread', { count: unread })}
            </p>
          )}
        </div>
        <PatternsBasis rows={rows} clock={clock} scope={scope} sample={sample} />
      </div>
    </TooltipProvider>
  );
}

function PatternRow({
  pattern,
  rows,
  clock,
  scope,
  selectedIndex,
  open,
  onOpenChange,
  onOpenLog,
  onFilter,
}: {
  pattern: LogPattern;
  rows: LogEntry[];
  clock: LogClock;
  scope: 'time' | 'day';
  selectedIndex: number | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onOpenLog: (index: number) => void;
  onFilter?: ((field: string, fragment: string) => void) | undefined;
}) {
  const { t } = useTranslation('logs');
  const examplesId = React.useId();
  const [shown, setShown] = React.useState(EXAMPLES);
  const percent = Math.round(pattern.share * 100);
  const [first, ...otherSources] = pattern.sources;
  // A log says its own level or source only where the pattern cannot say it for all of them.
  const mixedLevels = pattern.levels.length > 1;
  const mixedSources = pattern.sources.length > 1;

  return (
    <div data-log-pattern={pattern.key} data-log-level={pattern.tone}>
      <div
        onClick={() => onOpenChange(!open)}
        className={cn(
          'group relative min-h-[36px] cursor-pointer py-[6px] font-sans text-xs',
          ROW,
          LEVEL_EDGE[pattern.tone],
          open ? 'bg-bg-2' : 'hover:bg-bg-2',
        )}
      >
        {/* The row's own control, the size of the row: under the keyboard it takes the fill the pointer gives. */}
        <button
          type="button"
          aria-expanded={open}
          aria-controls={open ? examplesId : undefined}
          aria-label={t('explore.patterns.open_aria', { pattern: pattern.key, count: pattern.count })}
          className="absolute inset-0 focus-visible:bg-bg-3"
        />
        <span className={cn('relative font-mono text-md font-strong tabular-nums text-tx-0', CELL.count)}>
          {pattern.count}
        </span>
        {/* The count again as a length, of all the logs read: the number itself is said once. */}
        <span className={cn('relative', CELL.share)} title={`${percent}%`}>
          <span className="block h-[6px] overflow-hidden rounded-full bg-tx-0/[0.07]">
            <span
              data-log-pattern-share
              className="block h-full rounded-full bg-blue-soft"
              style={{ width: `${Math.max(2, pattern.share * 100)}%` }}
            />
          </span>
        </span>
        <span className={cn('relative flex items-center gap-1.5', CELL.pattern)}>
          <ChevronRight
            aria-hidden="true"
            className={cn('h-3 w-3 shrink-0 text-tx-3 transition-transform', open && 'rotate-90')}
          />
          <PatternText tokens={pattern.tokens} className={open ? 'whitespace-pre-wrap break-words' : 'truncate'} />
        </span>
        <PatternLevels levels={pattern.levels} className={cn('relative', CELL.level)} />
        <span
          className={cn('relative truncate text-tx-1', CELL.source)}
          title={mixedSources ? pattern.sources.map((source) => `${source.name} (${source.count})`).join(', ') : undefined}
        >
          {first ? first.name : '—'}
          {otherSources.length > 0 && <span className="text-tx-3"> +{otherSources.length}</span>}
        </span>
        <span className={cn('relative flex justify-end', CELL.action)}>
          {onFilter && pattern.fragment && (
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  aria-label={t('explore.patterns.filter_aria', { pattern: pattern.key })}
                  onClick={(event) => {
                    // Asking for the pattern is not opening it.
                    event.stopPropagation();
                    onFilter(pattern.field, pattern.fragment);
                  }}
                  className="grid h-7 w-7 place-items-center rounded-md text-tx-2 opacity-0 hover:bg-bg-4 hover:text-tx-0 focus-visible:bg-bg-4 focus-visible:text-tx-0 focus-visible:opacity-100 group-hover:opacity-100"
                >
                  <ListFilter className="h-3.5 w-3.5" aria-hidden="true" />
                </button>
              </TooltipTrigger>
              <TooltipContent>{t('explore.patterns.filter')}</TooltipContent>
            </Tooltip>
          )}
        </span>
      </div>
      {open && (
        <div id={examplesId} data-log-pattern-examples className="border-b border-bd-0 bg-bg-1 py-1 pl-[68px] pr-4">
          {pattern.rows.slice(0, shown).map((index) => {
            const log = rows[index]!;
            const selected = selectedIndex === index;

            return (
              <button
                key={index}
                type="button"
                data-log-pattern-example
                aria-pressed={selected}
                onClick={() => onOpenLog(index)}
                className={cn(
                  'flex w-full items-baseline gap-3 rounded-md px-2 py-[5px] text-left font-sans text-xs',
                  selected
                    ? 'bg-indigo-dim focus-visible:bg-indigo/25 [&:focus-visible_*]:text-tx-0'
                    : 'hover:bg-bg-3 focus-visible:bg-bg-3',
                )}
              >
                <span
                  className={cn(
                    'type-micro shrink-0 whitespace-nowrap font-mono tabular-nums text-tx-2',
                    scope === 'time' ? 'w-[92px]' : 'w-[132px]',
                  )}
                >
                  {log.micros == null ? log.ts : formatLogMoment(log.micros, clock, scope)}
                </span>
                {mixedLevels && (
                  <span className="w-[52px] shrink-0 truncate">
                    <LevelWord level={logLevelLabel(log.raw, log.level)} />
                  </span>
                )}
                <span className="min-w-0 flex-1 truncate text-tx-0">{logStatement(log.raw)?.value.split('\n', 1)[0]}</span>
                {mixedSources && (
                  <span className="hidden max-w-[152px] shrink-0 truncate text-tx-2 [@container_(min-width:800px)]:block">
                    {logSourceLabel(log.raw)}
                  </span>
                )}
              </button>
            );
          })}
          {pattern.rows.length > shown && (
            <button
              type="button"
              onClick={() => setShown(shown + EXAMPLES_STEP)}
              className="rounded-md px-2 py-[5px] font-sans text-xs text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
            >
              {t('explore.patterns.show_more', { count: Math.min(EXAMPLES_STEP, pattern.rows.length - shown) })}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * The levels of a pattern's logs, the loudest first, each with how many logs
 * carry it. A pattern of one level says only the level: the count is the
 * pattern's own. They stand on one line in whole words: what does not fit
 * wraps under the line and is not shown, and the title has all of it.
 */
function PatternLevels({ levels, className }: { levels: LogPattern['levels']; className?: string }) {
  const sorted = [...levels].sort(
    (left, right) => LEVEL_RANK[logLevelTone(right.level)] - LEVEL_RANK[logLevelTone(left.level)],
  );
  const mixed = sorted.length > 1;
  const rest = sorted.length - LEVELS_NAMED;

  return (
    <span
      data-log-pattern-levels
      className={cn('flex h-[16px] flex-wrap items-baseline gap-x-2 overflow-hidden', className)}
      title={mixed ? sorted.map(({ level, count }) => `${level} ${count}`).join(' · ') : undefined}
    >
      {sorted.slice(0, LEVELS_NAMED).map(({ level, count }) => (
        <span key={level} className="inline-flex h-[16px] shrink-0 items-baseline gap-1 whitespace-nowrap leading-[16px]">
          <LevelWord level={level} />
          {mixed && <span className="type-micro font-mono tabular-nums text-tx-3">{count}</span>}
        </span>
      ))}
      {rest > 0 && (
        <span className="type-micro h-[16px] shrink-0 font-mono tabular-nums leading-[16px] text-tx-3">+{rest}</span>
      )}
    </span>
  );
}

/** A pattern as it is read: its words as words, and an open slot where every log says something of its own. */
function PatternText({ tokens, className }: { tokens: readonly PatternToken[]; className?: string }) {
  const { t } = useTranslation('logs');

  return (
    <span data-log-pattern-text className={cn('min-w-0 text-tx-0', className)}>
      {tokens.map((token, index) => (
        <React.Fragment key={index}>
          {index > 0 && ' '}
          {token.lead}
          {token.wild ? (
            <span
              data-log-pattern-slot={token.wild}
              title={`${t(`explore.patterns.kind.${token.wild}`)}: ${token.text}`}
              className="rounded-[3px] bg-tx-0/[0.08] px-[4px] font-mono text-tx-2"
            >
              *
            </span>
          ) : (
            token.text
          )}
          {token.unit}
          {token.trail}
        </React.Fragment>
      ))}
    </span>
  );
}

/**
 * What the patterns were read from, where the list has its pages: which logs
 * they are and how far back they reach, and, while older ones match, the way
 * to read further.
 */
function PatternsBasis({
  rows,
  clock,
  scope,
  sample,
}: {
  rows: LogEntry[];
  clock: LogClock;
  scope: 'time' | 'day';
  sample: PatternSampleState | undefined;
}) {
  const { t } = useTranslation('logs');
  const { t: tCommon } = useTranslation('common');
  // The rows come newest first: the last one with a moment is how far back they reach.
  const oldest = React.useMemo(() => {
    for (let index = rows.length - 1; index >= 0; index -= 1) {
      const micros = rows[index]!.micros;
      if (micros != null) return micros;
    }

    return null;
  }, [rows]);
  const partial = sample !== undefined && (sample.more || sample.reading || sample.failed);

  return (
    <div
      data-log-patterns-basis
      className="flex min-h-11 shrink-0 flex-wrap items-center gap-x-2 gap-y-1 px-4 py-1.5 font-sans text-xs text-tx-2"
    >
      {/* Until the first answer the rows are the page the list had, which need not be the latest. */}
      {(!sample || sample.held) && (
        <>
          <span>
            {t(
              !sample
                ? 'explore.patterns.basis_returned'
                : partial
                  ? 'explore.patterns.basis_latest'
                  : 'explore.patterns.basis_all',
            )}
          </span>
          {partial && oldest != null && (
            <>
              <span aria-hidden="true" className="text-tx-3">
                ·
              </span>
              <span className="whitespace-nowrap tabular-nums">
                {t('explore.patterns.basis_back_to', { moment: formatLogMoment(oldest, clock, scope) })}
              </span>
            </>
          )}
        </>
      )}
      {sample && (
        <span className="ml-auto flex shrink-0 items-center gap-2">
          {sample.reading ? (
            <span role="status" className="inline-flex items-center gap-1.5">
              <Loader2 aria-hidden="true" className="h-3.5 w-3.5 animate-spin text-tx-3" />
              {t('explore.patterns.reading')}
            </span>
          ) : sample.failed ? (
            <>
              <span role="alert" className="text-[var(--red-ink)]">
                {t('explore.patterns.read_failed')}
              </span>
              <ChromeButton size="sm" onClick={sample.onRetry}>
                {tCommon('actions.retry')}
              </ChromeButton>
            </>
          ) : sample.more ? (
            <ChromeButton size="sm" onClick={sample.onReadMore}>
              {t('explore.patterns.read_more', { logs: sample.step })}
            </ChromeButton>
          ) : null}
        </span>
      )}
    </div>
  );
}
