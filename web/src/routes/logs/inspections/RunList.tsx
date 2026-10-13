import { useTranslation } from 'react-i18next';

import type { LogInspectionRun } from '@/api/logInspections';
import { formatMicrosActive } from '@/lib/time';
import { Pill } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { formatRelativeMicros } from '@/time/relative';

import { LevelMark, RunStatusMark } from './LevelMark';

/** A run by what it came to: the verdict of its report, or how it stands without one. */
export function RunMark({ run }: { run: LogInspectionRun }) {
  return run.status === 'succeeded' ? (
    <LevelMark level={run.level ?? 'unknown'} />
  ) : (
    <RunStatusMark status={run.status} />
  );
}

interface RunListProps {
  runs: readonly LogInspectionRun[];
  selectedId: string | null;
  /** The moment the list was read at: times are told from it. */
  nowMicros: number;
  /** There may be older runs than the ones listed. */
  more: boolean;
  onSelect: (run: LogInspectionRun) => void;
  onMore: () => void;
}

/**
 * The runs of an inspection, the most recent first. A row is what a run came
 * to and when, and the first words of its report, so the one worth opening can
 * be told from the list.
 */
export function RunList({ runs, selectedId, nowMicros, more, onSelect, onMore }: RunListProps) {
  const { t, i18n } = useTranslation('logs');
  const locale = i18n.resolvedLanguage ?? i18n.language;

  return (
    <nav aria-label={t('inspections.detail.runs')} className="flex min-h-0 flex-col">
      <ul className="m-0 flex list-none flex-col gap-[2px] p-[6px]">
        {runs.map((run) => {
          const selected = run.id === selectedId;
          const line =
            run.summary ?? (run.failure ? t(`inspections.failures.${run.failure}`) : null);

          return (
            <li key={run.id}>
              <button
                type="button"
                data-log-inspection-run={run.id}
                data-log-inspection-run-status={run.status}
                aria-current={selected ? 'true' : undefined}
                onClick={() => onSelect(run)}
                className={cn(
                  'flex w-full min-w-0 flex-col gap-[4px] rounded-md px-[10px] py-[8px] text-left font-sans transition-colors hover:bg-bg-2 focus-visible:bg-bg-3 focus-visible:outline-none',
                  selected && 'bg-bg-3 hover:bg-bg-3',
                )}
              >
                <span className="flex min-w-0 items-center gap-[8px]">
                  <RunMark run={run} />
                  {run.trigger === 'manual' && (
                    <Pill tone="neutral">{t('inspections.triggers.manual')}</Pill>
                  )}
                  <span
                    className="ml-auto shrink-0 whitespace-nowrap text-xs text-tx-2"
                    title={formatMicrosActive(run.created_at_micros)}
                  >
                    {formatRelativeMicros(run.created_at_micros, locale, nowMicros)}
                  </span>
                </span>
                {line && <span className="line-clamp-2 text-xs leading-relaxed text-tx-2">{line}</span>}
              </button>
            </li>
          );
        })}
      </ul>
      {more && (
        <button
          type="button"
          onClick={onMore}
          className="mx-[6px] mb-[6px] h-8 rounded-md bg-bg-2 font-sans text-xs font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:outline-none"
        >
          {t('inspections.detail.older')}
        </button>
      )}
    </nav>
  );
}
