import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { LEVEL_EDGE } from '../LevelMark';
import {
  formatLogMoment,
  logLevelLabel,
  logLevelTone,
  logSourceLabel,
  primaryLogMessage,
  type LogClock,
  type LogEntry,
} from '../viewModel';

interface LogDetailContextProps {
  rows: LogEntry[];
  current: LogEntry;
  clock: LogClock;
  onSelect: (log: LogEntry) => void;
}

/**
 * The results on either side of the one that is open, as the list has them:
 * what happened just before and just after. Picking one opens it.
 */
export function LogDetailContext({ rows, current, clock, onSelect }: LogDetailContextProps) {
  const { t } = useTranslation('logs');

  return (
    <div>
      <p className="pb-2 font-sans text-xs leading-5 text-tx-2">{t('explore.detail.context_description')}</p>
      <div>
        {rows.map((log, index) => {
          const active = log === current;
          const message = primaryLogMessage(log.raw).value;

          return (
            <button
              key={`${log.ts}-${index}`}
              type="button"
              aria-current={active ? 'true' : undefined}
              onClick={() => onSelect(log)}
              className={cn(
                'grid w-full grid-cols-[92px_minmax(0,1fr)] items-baseline gap-3 border-b border-l-[3px] border-bd-0 py-[7px] pl-[10px] pr-2 text-left font-sans text-xs',
                LEVEL_EDGE[logLevelTone(logLevelLabel(log.raw, log.level))],
                active ? 'bg-indigo-dim' : 'hover:bg-bg-2 focus-visible:bg-bg-2',
              )}
            >
              <span className="type-micro whitespace-nowrap font-mono tabular-nums text-tx-2">
                {log.micros == null ? log.ts : formatLogMoment(log.micros, clock, 'time')}
              </span>
              <span className="min-w-0">
                <span className="block truncate text-tx-0">{message}</span>
                <span className="type-micro mt-[1px] block truncate text-tx-2">{logSourceLabel(log.raw)}</span>
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
