import { Loader2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { LogPatternBaseline, LogPatternReport } from '@/api/web';
import { ChromeButton } from '@/shell/chrome';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { formatLogMoment, type LogClock, type LogEntry } from '../viewModel';

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

/** The patterns of the whole range as the server counts them, and what they are compared with. */
export interface ServerPatternsState {
  report: LogPatternReport | undefined;
  loading: boolean;
  failed: boolean;
  onRetry: () => void;
  baseline: LogPatternBaseline;
  onBaselineChange: (baseline: LogPatternBaseline) => void;
}

const BASELINES: readonly LogPatternBaseline[] = ['previous', 'week_before', 'none'];

/**
 * What the patterns were read from. Where the server counted them: how many
 * events of the range they are of, and how many it read when it sampled, with
 * what they are compared with. Else, where the list has its pages: which logs
 * they are and how far back they reach. While older logs match, the way to
 * read further, for more of each pattern's logs to open.
 */
export function PatternsBasis({
  rows,
  clock,
  scope,
  sample,
  server,
}: {
  rows: LogEntry[];
  clock: LogClock;
  scope: 'time' | 'day';
  sample: PatternSampleState | undefined;
  server: ServerPatternsState | undefined;
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
  const report = server?.report;

  return (
    <div
      data-log-patterns-basis
      className="flex min-h-11 shrink-0 flex-wrap items-center gap-x-2 gap-y-1 px-4 py-1.5 font-sans text-xs text-tx-2"
    >
      {report ? (
        <span data-log-patterns-counted>
          {t('explore.patterns.basis_counted', { count: report.logs, formatted: report.logs.toLocaleString() })}
          {report.sampled && (
            <>
              <span aria-hidden="true" className="text-tx-3">
                {' · '}
              </span>
              {t('explore.patterns.basis_sampled', { read: report.read.toLocaleString() })}
            </>
          )}
        </span>
      ) : (
        // Until the first answer the rows are the page the list had, which need not be the latest.
        (!sample || sample.held) && (
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
        )
      )}
      <span className="ml-auto flex shrink-0 items-center gap-2">
        {server &&
          (server.loading ? (
            <span role="status" className="inline-flex items-center gap-1.5">
              <Loader2 aria-hidden="true" className="h-3.5 w-3.5 animate-spin text-tx-3" />
              {t('explore.patterns.counting')}
            </span>
          ) : server.failed ? (
            <>
              <span role="alert" className="text-[var(--red-ink)]">
                {t('explore.patterns.count_failed')}
              </span>
              <ChromeButton size="sm" onClick={server.onRetry}>
                {tCommon('actions.retry')}
              </ChromeButton>
            </>
          ) : null)}
        {/* A baseline is the server's: the logs read alone are compared with nothing. */}
        {server && report && (
          <FittedSelect
            aria-label={t('explore.patterns.baseline_label')}
            value={server.baseline}
            onValueChange={(value) => server.onBaselineChange(value as LogPatternBaseline)}
            options={BASELINES.map((baseline) => ({ value: baseline, label: t(`explore.patterns.baseline.${baseline}`) }))}
            maxWidth={240}
          />
        )}
        {sample &&
          (sample.reading ? (
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
          ) : null)}
      </span>
    </div>
  );
}
