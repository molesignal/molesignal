import { ChevronDown, ChevronUp, Download, ExternalLink, X, type LucideIcon } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { writeClipboardText } from '@/lib/clipboard';
import { useTimeFormatter } from '@/lib/time';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shell/ui/tooltip';
import { TraceView } from '@/viz/trace/view/TraceView';

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
import { LogDetailFields, LogFieldActions } from './Fields';
import { LogFieldValue } from './FieldValue';
import { JsonTree } from './JsonTree';
import { keyAttributes, logTraceOf } from './keyAttributes';
import { logFieldRows, logFileName, logRecordJson } from './model';

type DetailView = 'fields' | 'trace' | 'context' | 'json';

/** An hour on either side of the log: where the spans of its trace are looked for. */
const TRACE_WINDOW_MICROS = 3_600_000_000;

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
 * One log, read on its own, as Datadog's log panel reads it.
 *
 * What it is comes first and is said once: its level and its moment in the
 * head, then what it says, in full, and the attributes it is told by (its
 * service, host, environment, version, trace), each a reference with the
 * same ways on as anywhere else. Under that is the rest in the view the
 * reader picks: its attributes field by field, the trace it was written in,
 * the results around it, or the record as it was stored. Stepping to the
 * next log, copying and closing share the head's one row.
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
  const attributes = React.useMemo(() => {
    const found = keyAttributes(record);
    // A log with no service field is told by where it came from instead.
    return !found.some((attribute) => attribute.key === 'service') && source ? [{ key: 'source', field: source.field, value: source.value }, ...found] : found;
  }, [record, source]);
  // What the head says is not said again under it: the attributes it shows leave the field list.
  const rows = React.useMemo(() => {
    const shown = new Set(attributes.map((attribute) => attribute.field));
    return logFieldRows(record, statement?.field ?? null).filter((row) => !shown.has(row.path));
  }, [attributes, record, statement?.field]);
  const trace = React.useMemo(() => logTraceOf(record), [record]);
  const jump = React.useCallback((to: string) => navigate(to), [navigate]);
  // Another log without a trace has no trace tab: the view goes back to its attributes.
  const shownView: DetailView = view === 'trace' && !trace ? 'fields' : view;

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
            {trace && (
              <HeadButton label={t('explore.detail.open_trace')} icon={ExternalLink} onClick={() => navigate(`/traces/${encodeURIComponent(trace.traceId)}`)} />
            )}
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

        <div className="shrink-0 px-4 pb-3">
          {statement && (
            <div
              data-log-detail-message
              className="max-h-[34vh] overflow-y-auto whitespace-pre-wrap break-words rounded-md bg-[var(--control-surface)] px-3 py-[10px] font-mono text-xs leading-[1.65] text-tx-0 [overflow-wrap:anywhere]"
            >
              {statement.value}
            </div>
          )}
          <dl data-log-detail-attributes className="mt-2 flex min-w-0 flex-wrap items-center gap-1.5 font-sans text-xs">
            {attributes.map((attribute) => (
              <div
                key={attribute.field}
                data-log-detail-attribute={attribute.key}
                className="group relative flex min-w-0 max-w-full items-center gap-1.5 rounded-md bg-[var(--control-surface)] px-2 py-[3px] hover:bg-bg-3 has-[:focus-visible]:bg-bg-3"
              >
                <dt className="shrink-0 text-tx-3">{attribute.field}</dt>
                <dd className="min-w-0 truncate text-tx-0">
                  <LogFieldValue field={attribute.field} value={attribute.value} record={record} time={signalTime} onJump={jump} />
                </dd>
                {/* The same ways on as a row of the field list, over the chip while it is pointed at. */}
                <LogFieldActions
                  path={attribute.field}
                  value={attribute.value}
                  column={visibleFields.includes(attribute.field)}
                  onInclude={(field, value) => onInsertValueFilter(field, value, 'include')}
                  onExclude={(field, value) => onInsertValueFilter(field, value, 'exclude')}
                  onToggleVisibility={onToggleVisibility}
                  className="absolute bottom-full right-0 mb-0.5 shadow-sm"
                />
              </div>
            ))}
            <div className="flex min-w-0 items-center gap-1.5 rounded-md bg-[var(--control-surface)] px-2 py-[3px]">
              <dt className="shrink-0 text-tx-3">{t('explore.detail.stream')}</dt>
              <dd className="min-w-0 truncate text-tx-1">{stream}</dd>
            </div>
          </dl>
        </div>

        <div className="shrink-0 border-b border-bd-0 px-4 pb-2">
          <ViewTabs
            label={t('explore.detail.views_aria')}
            items={[
              { key: 'fields', label: t('explore.detail.tabs.fields'), count: rows.length },
              ...(trace ? [{ key: 'trace' as const, label: t('explore.detail.tabs.trace') }] : []),
              { key: 'context', label: t('explore.detail.tabs.context') },
              { key: 'json', label: t('explore.detail.tabs.json') },
            ]}
            value={shownView}
            onValueChange={setView}
          />
        </div>

        {shownView === 'trace' && trace ? (
          // The trace takes the rest of the drawer: its waterfall scrolls itself.
          <div data-log-detail-trace className="min-h-0 min-w-0 flex-1 overflow-hidden">
            <TraceView
              key={trace.traceId}
              traceId={trace.traceId}
              window={log.micros == null ? undefined : { from: log.micros - TRACE_WINDOW_MICROS, to: log.micros + TRACE_WINDOW_MICROS }}
              initialSpanId={trace.spanId}
              frame="frame"
            />
          </div>
        ) : (
          <div data-log-detail-scroll className="min-h-0 min-w-0 flex-1 overflow-y-auto overflow-x-hidden px-4 py-3">
            {shownView === 'fields' && (
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
            {shownView === 'json' && <JsonTree obj={record} time={signalTime} onJump={jump} />}
            {shownView === 'context' && (
              <LogDetailContext rows={contextRows} current={log} clock={clock} onSelect={onSelectContext} />
            )}
          </div>
        )}
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
