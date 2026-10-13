import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { useTimeFormatter } from '@/lib/time';
import { uiTableHeaderClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { LONG_LIST_ROW_CLASS, longListRowStyle } from '@/shell/list/longList';

import { LEVEL_EDGE, LevelWord } from './LevelMark';
import {
  displayLogValue,
  formatLogMoment,
  logLevelTone,
  LOG_RESULT_ROW_HEIGHT,
  momentsShareDay,
  type LogClock,
  type LogEntry,
} from './viewModel';

const TIMESTAMP_FIELDS = new Set(['_timestamp', 'timestamp', 'time']);
const LEVEL_FIELDS = new Set(['level', 'severity', 'severity_text', 'log.level']);
const MESSAGE_FIELDS = new Set(['message', 'body']);
const SERVICE_FIELDS = new Set(['service', 'service.name', 'service_name', 'source']);

interface ColumnWidthBounds {
  min: number;
  max: number;
}

/** The time of day to the millisecond at the least; the day of the year before it at the most. */
const TIME_COLUMN_WIDTH: ColumnWidthBounds = { min: 92, max: 160 };
/** The message is what a log is read for: it is never squeezed under this, and takes the room the others leave. */
const MESSAGE_MIN_WIDTH = 200;
/** How many rows a column's width is judged by: the widths are capped, and a page can hold thousands. */
const WIDTH_SAMPLE_ROWS = 300;

function visibleResultFields(fields: string[]): string[] {
  return fields.filter((field) => !TIMESTAMP_FIELDS.has(field));
}

function columnWidthBounds(field: string): ColumnWidthBounds {
  if (LEVEL_FIELDS.has(field)) return { min: 56, max: 72 };
  if (MESSAGE_FIELDS.has(field)) return { min: MESSAGE_MIN_WIDTH, max: 300 };
  if (field === 'error') return { min: 96, max: 220 };
  if (SERVICE_FIELDS.has(field)) return { min: 84, max: 132 };
  if (field.endsWith('_id') || field.endsWith('.id')) return { min: 96, max: 160 };
  return { min: 80, max: 144 };
}

function logResultRawValue(log: LogEntry, field: string): unknown {
  if (Object.prototype.hasOwnProperty.call(log.raw, field)) return log.raw[field];
  return LEVEL_FIELDS.has(field) ? log.level : undefined;
}

function estimatedTextWidth(value: string): number {
  const contentWidth = Array.from(value).reduce((width, character) => (
    width + (character.codePointAt(0)! > 0xff ? 12 : 7)
  ), 0);
  return contentWidth + 20;
}

function preferredColumnWidth(
  field: string,
  rows: LogEntry[],
  bounds: ColumnWidthBounds,
): number {
  const contentWidth = rows.slice(0, WIDTH_SAMPLE_ROWS).reduce((width, log) => Math.max(
    width,
    estimatedTextWidth(displayLogValue(logResultRawValue(log, field))),
  ), estimatedTextWidth(field));
  return Math.min(bounds.max, Math.max(bounds.min, contentWidth));
}

/**
 * The columns of the list. Every column is as wide as what it holds, up to a
 * cap, and cuts what does not fit; the first message column is the exception:
 * it takes whatever room the others leave, since the room would otherwise
 * stand empty beside a message cut short.
 */
function resultGridStyle(
  fields: string[],
  rows: LogEntry[],
  timeLabels: string[],
): React.CSSProperties {
  const flexible = fields.find((field) => MESSAGE_FIELDS.has(field));
  const widths = fields.map((field) => {
    const bounds = columnWidthBounds(field);
    return { field, ...bounds, preferred: preferredColumnWidth(field, rows, bounds) };
  });
  const timePreferredWidth = Math.min(
    TIME_COLUMN_WIDTH.max,
    Math.max(TIME_COLUMN_WIDTH.min, ...timeLabels.slice(0, WIDTH_SAMPLE_ROWS).map(estimatedTextWidth)),
  );
  const tracks = widths.map(({ field, min, preferred }) => (
    field === flexible ? `minmax(${min}px, 1fr)` : `minmax(${min}px, ${preferred}px)`
  ));
  const minimumContentWidth = TIME_COLUMN_WIDTH.min
    + widths.reduce((sum, width) => sum + width.min, 0)
    + fields.length * 12;
  return {
    gridTemplateColumns: [`minmax(${TIME_COLUMN_WIDTH.min}px, ${timePreferredWidth}px)`, ...tracks].join(' '),
    minWidth: `${minimumContentWidth + 32}px`,
  };
}

function LogResultValue({ log, field }: { log: LogEntry; field: string }) {
  const rawValue = logResultRawValue(log, field);
  const value = displayLogValue(rawValue);
  if (LEVEL_FIELDS.has(field) && value) return <LevelWord level={value.toUpperCase()} />;

  return (
    <span className="block truncate" title={value}>
      {value || '—'}
    </span>
  );
}

interface LogRowProps {
  log: LogEntry;
  index: number;
  moment: string;
  selected: boolean;
  columns: string[];
  style: React.CSSProperties;
  clock: LogClock;
  label: (index: number) => string;
  onSelect: (index: number) => void;
}

/**
 * One log of the list. It is drawn again only when it changes itself: opening
 * a log redraws the row that was open and the one that is, not the thousands
 * between them.
 */
const LogRow = React.memo(function LogRow({
  log,
  index,
  moment,
  selected,
  columns,
  style,
  clock,
  label,
  onSelect,
}: LogRowProps) {
  const tone = logLevelTone(log.level);

  return (
    <div
      data-log-result-row="logs"
      data-log-level={tone}
      role="button"
      tabIndex={0}
      aria-label={label(index)}
      aria-pressed={selected}
      onClick={() => onSelect(index)}
      onKeyDown={(event) => {
        if (event.key === 'Enter' || event.key === ' ') {
          event.preventDefault();
          onSelect(index);
        }
      }}
      style={style}
      className={cn(
        // The level runs down the row's edge whether or not it has a column of its own.
        'grid cursor-pointer items-center gap-3 border-b border-l-[3px] border-bd-0 pl-[13px] pr-4 text-left font-sans text-xs',
        LONG_LIST_ROW_CLASS,
        LEVEL_EDGE[tone],
        selected ? 'bg-indigo-dim' : 'hover:bg-bg-2 focus-visible:bg-bg-2',
      )}
    >
      <span
        data-log-field="_timestamp"
        className="type-micro min-w-0 truncate whitespace-nowrap font-mono tabular-nums text-tx-2"
        title={log.micros == null ? log.ts : formatLogMoment(log.micros, clock, 'full')}
      >
        {moment}
      </span>
      {columns.map((field) => (
        <span key={field} data-log-field={field} className="min-w-0 text-tx-1">
          <LogResultValue log={log} field={field} />
        </span>
      ))}
    </div>
  );
});

interface LogListResultsProps {
  rows: LogEntry[];
  fields: string[];
  startIndex: number;
  selectedIndex: number | null;
  onSelect: (index: number) => void;
}

export function LogListResults({
  rows,
  fields,
  startIndex,
  selectedIndex,
  onSelect,
}: LogListResultsProps) {
  const { t } = useTranslation('logs');
  const time = useTimeFormatter();
  const clock = React.useMemo<LogClock>(
    () => ({ tz: time.tz, format: time.format, dateFormat: time.dateFormat }),
    [time.dateFormat, time.format, time.tz],
  );
  const columns = React.useMemo(() => visibleResultFields(fields), [fields]);
  const timeColumnLabel = t('explore.table.time_column');
  // The day is said on every row only where the rows are of more than one.
  const moments = React.useMemo(() => {
    const scope = momentsShareDay(rows.map((log) => log.micros), clock.tz) ? 'time' : 'day';

    return rows.map((log) => (log.micros == null ? log.ts : formatLogMoment(log.micros, clock, scope)));
  }, [clock, rows]);
  const gridStyle = React.useMemo(
    () => resultGridStyle(columns, rows, [timeColumnLabel, ...moments]),
    [columns, moments, rows, timeColumnLabel],
  );
  const rowStyle = React.useMemo(
    // A row is one height, the line under it included.
    () => ({ ...gridStyle, minHeight: LOG_RESULT_ROW_HEIGHT, ...longListRowStyle(LOG_RESULT_ROW_HEIGHT - 1) }),
    [gridStyle],
  );
  const rowLabel = React.useCallback((index: number) => t('explore.table.row_aria', { index: index + 1 }), [t]);

  return (
    <div className="h-full min-h-0 overflow-auto">
      <div
        data-log-result-columns="logs"
        style={gridStyle}
        // The same edge as the rows under it, so the columns line up.
        className={`sticky top-0 z-10 grid gap-3 border-b border-l-[3px] border-bd-0 border-l-transparent bg-bg-1 py-2 pl-[13px] pr-4 ${uiTableHeaderClass}`}
      >
        <span data-log-field="_timestamp" className="min-w-0 truncate" title={timeColumnLabel}>
          {timeColumnLabel}
        </span>
        {columns.map((field) => (
          <span key={field} data-log-field={field} className="min-w-0 truncate" title={field}>
            {field}
          </span>
        ))}
      </div>
      {rows.map((log, pageIndex) => {
        const index = startIndex + pageIndex;

        return (
          <LogRow
            key={index}
            log={log}
            index={index}
            moment={moments[pageIndex]!}
            selected={index === selectedIndex}
            columns={columns}
            style={rowStyle}
            clock={clock}
            label={rowLabel}
            onSelect={onSelect}
          />
        );
      })}
    </div>
  );
}
