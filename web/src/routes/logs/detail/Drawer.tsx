import { ChevronDown, ChevronUp, Download, X, type LucideIcon } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { writeClipboardText } from '@/lib/clipboard';
import { useTimeFormatter } from '@/lib/time';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shell/ui/tooltip';

import { LevelMark } from '../LevelMark';
import { signalTimeAround } from '../relatedSignals';
import {
  formatLogMoment,
  logLevelLabel,
  logSource,
  logStatement,
  type LogClock,
  type LogEntry,
} from '../viewModel';
import { LogDetailContext } from './Context';
import { LogDetailFields } from './Fields';
import { LogFieldValue } from './FieldValue';
import { JsonTree } from './JsonTree';
import { logFieldRows, logFileName, logRecordJson } from './model';

type DetailView = 'fields' | 'json' | 'context';

interface LogDetailProps {
  log: LogEntry;
  index: number;
  total: number;
  stream: string;
  /** The results on either side of this one, this one among them. */
  contextRows: LogEntry[];
  visibleFields: string[];
  onInsertValueFilter: (field: string, value: unknown, mode: 'include' | 'exclude') => void;
  onToggleVisibility: (field: string) => void;
  onSelectContext: (log: LogEntry) => void;
  onClose: () => void;
  onPrev: () => void;
  onNext: () => void;
}

/**
 * One log, read on its own.
 *
 * What it is comes first and is said once: its level and its moment in the
 * head, then what it says, in full, and where it came from. Under that is the
 * rest of the record, in the view the reader picks: field by field, as it was
 * stored, or among the results around it. Stepping to the next log, copying
 * and closing share the head's one row.
 */
export function LogDetail({
  log,
  index,
  total,
  stream,
  contextRows,
  visibleFields,
  onInsertValueFilter,
  onToggleVisibility,
  onSelectContext,
  onClose,
  onPrev,
  onNext,
}: LogDetailProps) {
  const { t } = useTranslation('logs');
  const navigate = useNavigate();
  const time = useTimeFormatter();
  const clock = React.useMemo<LogClock>(
    () => ({ tz: time.tz, format: time.format, dateFormat: time.dateFormat }),
    [time.dateFormat, time.format, time.tz],
  );
  const rootRef = React.useRef<HTMLDivElement>(null);
  const [view, setView] = React.useState<DetailView>('fields');
  const [copied, setCopied] = React.useState(false);
  const copyTimer = React.useRef<number | null>(null);
  const record = log.raw;

  // Opened from the list, the drawer takes the keyboard: Escape closes it from the start.
  React.useEffect(() => {
    rootRef.current?.focus();

    return () => {
      if (copyTimer.current !== null) window.clearTimeout(copyTimer.current);
    };
  }, []);

  const statement = logStatement(record);
  const source = logSource(record);
  const level = logLevelLabel(record, log.level);
  const signalTime = signalTimeAround(log.micros);
  const rows = React.useMemo(() => logFieldRows(record, statement?.field ?? null), [record, statement?.field]);
  const jump = React.useCallback((to: string) => navigate(to), [navigate]);

  const copy = async () => {
    await writeClipboardText(logRecordJson(record));
    setCopied(true);
    if (copyTimer.current !== null) window.clearTimeout(copyTimer.current);
    copyTimer.current = window.setTimeout(() => setCopied(false), 1400);
  };

  return (
    <TooltipProvider delayDuration={250}>
      <div
        ref={rootRef}
        tabIndex={-1}
        data-log-detail
        onKeyDown={(event) => {
          // A menu opened from a value closes on Escape first; the drawer takes the next one.
          if (event.key !== 'Escape' || document.querySelector('[data-radix-popper-content-wrapper]')) return;
          event.stopPropagation();
          onClose();
        }}
        className="flex h-full min-h-0 flex-col overflow-hidden bg-[var(--functional-surface)]"
      >
        <header className="flex h-[48px] shrink-0 items-center gap-3 pl-4 pr-2">
          <LevelMark level={level} />
          <time data-log-detail-moment className="min-w-0 truncate font-mono text-xs tabular-nums text-tx-1">
            {log.micros == null ? log.ts : formatLogMoment(log.micros, clock, 'full')}
          </time>
          <div className="ml-auto flex shrink-0 items-center">
            <HeadButton label={t('explore.detail.prev')} icon={ChevronUp} disabled={index === 0} onClick={onPrev} />
            <span
              aria-label={t('explore.detail.position', { current: index + 1, total })}
              className="min-w-[44px] px-[2px] text-center font-mono text-xs tabular-nums text-tx-2"
            >
              {index + 1} / {total}
            </span>
            <HeadButton
              label={t('explore.detail.next')}
              icon={ChevronDown}
              disabled={index === total - 1}
              onClick={onNext}
            />
            <span aria-hidden="true" className="mx-[6px] h-4 w-px bg-bd-1" />
            <CopyIconButton
              label={t('explore.detail.copy_json')}
              copied={copied}
              copiedLabel={t('explore.detail.copied_json')}
              onClick={() => void copy()}
            />
            <HeadButton
              label={t('explore.detail.download_json')}
              icon={Download}
              onClick={() => downloadJson(logFileName(log.ts, index + 1), record)}
            />
            <HeadButton label={t('explore.detail.close')} icon={X} onClick={onClose} />
          </div>
        </header>

        {(statement || source) && (
          <div className="shrink-0 px-4 pb-3">
            {statement && (
              <div
                data-log-detail-message
                className="max-h-[34vh] overflow-y-auto whitespace-pre-wrap break-words rounded-md bg-[var(--control-surface)] px-3 py-[10px] font-sans text-sm leading-[1.65] text-tx-0 [overflow-wrap:anywhere]"
              >
                {statement.value}
              </div>
            )}
            <div className="mt-2 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 font-sans text-xs text-tx-2">
              {source && (
                <>
                  <LogFieldValue
                    field={source.field}
                    value={source.value}
                    record={record}
                    time={signalTime}
                    onJump={jump}
                    className="text-tx-1"
                  />
                  <span aria-hidden="true" className="text-tx-4">·</span>
                </>
              )}
              <span className="min-w-0 truncate">{stream}</span>
            </div>
          </div>
        )}

        <div className="shrink-0 border-b border-bd-0 px-4 pb-2">
          <ViewTabs
            label={t('explore.detail.views_aria')}
            items={[
              { key: 'fields', label: t('explore.detail.tabs.fields'), count: rows.length },
              { key: 'json', label: t('explore.detail.tabs.json') },
              { key: 'context', label: t('explore.detail.tabs.context') },
            ]}
            value={view}
            onValueChange={setView}
          />
        </div>

        <div data-log-detail-scroll className="min-h-0 min-w-0 flex-1 overflow-y-auto overflow-x-hidden px-4 py-3">
          {view === 'fields' && (
            <LogDetailFields
              rows={rows}
              record={record}
              time={signalTime}
              visibleFields={visibleFields}
              onInclude={(field, value) => onInsertValueFilter(field, value, 'include')}
              onExclude={(field, value) => onInsertValueFilter(field, value, 'exclude')}
              onToggleVisibility={onToggleVisibility}
              onJump={jump}
            />
          )}
          {view === 'json' && <JsonTree obj={record} time={signalTime} onJump={jump} />}
          {view === 'context' && (
            <LogDetailContext rows={contextRows} current={log} clock={clock} onSelect={onSelectContext} />
          )}
        </div>
      </div>
    </TooltipProvider>
  );
}

function HeadButton({
  label,
  icon: Icon,
  onClick,
  disabled = false,
}: {
  label: string;
  icon: LucideIcon;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={label}
          disabled={disabled}
          onClick={onClick}
          className="grid h-8 w-8 place-items-center rounded-md text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent disabled:hover:text-tx-2"
        >
          <Icon className="h-3.5 w-3.5" aria-hidden="true" />
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function downloadJson(filename: string, value: unknown): void {
  const blob = new Blob([JSON.stringify(value, null, 2)], { type: 'application/json;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');

  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}
