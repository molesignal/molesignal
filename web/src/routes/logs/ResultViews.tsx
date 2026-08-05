import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { uiTableHeaderClass } from '@/shell/chrome';

import {
  displayLogValue,
  logResultRowHeight,
  type LogEntry,
  type LogResultDensity,
} from './viewModel';

const TIMESTAMP_FIELDS = new Set(['_timestamp', 'timestamp', 'time']);
const LEVEL_FIELDS = new Set(['level', 'severity', 'severity_text', 'log.level']);
const MESSAGE_FIELDS = new Set(['message', 'body']);
const SERVICE_FIELDS = new Set(['service', 'service.name', 'service_name', 'source']);

interface ColumnWidthBounds {
  min: number;
  max: number;
}

const TIME_COLUMN_WIDTH: ColumnWidthBounds = { min: 132, max: 160 };

function visibleResultFields(fields: string[]): string[] {
  return fields.filter((field) => !TIMESTAMP_FIELDS.has(field));
}

function columnWidthBounds(field: string): ColumnWidthBounds {
  if (LEVEL_FIELDS.has(field)) return { min: 56, max: 72 };
  if (MESSAGE_FIELDS.has(field)) return { min: 144, max: 300 };
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
  const contentWidth = rows.reduce((width, log) => Math.max(
    width,
    estimatedTextWidth(displayLogValue(logResultRawValue(log, field))),
  ), estimatedTextWidth(field));
  return Math.min(bounds.max, Math.max(bounds.min, contentWidth));
}

function resultGridStyle(
  fields: string[],
  rows: LogEntry[],
  timeColumnLabel: string,
): React.CSSProperties {
  const widths = fields.map((field) => {
    const bounds = columnWidthBounds(field);
    return { ...bounds, preferred: preferredColumnWidth(field, rows, bounds) };
  });
  const timePreferredWidth = Math.min(
    TIME_COLUMN_WIDTH.max,
    Math.max(
      TIME_COLUMN_WIDTH.min,
      estimatedTextWidth(timeColumnLabel),
      ...rows.map((log) => estimatedTextWidth(log.ts)),
    ),
  );
  const tracks = widths.map(({ min, preferred }) => `minmax(${min}px, ${preferred}px)`);
  const minimumContentWidth = TIME_COLUMN_WIDTH.min
    + widths.reduce((sum, width) => sum + width.min, 0)
    + fields.length * 12;
  return {
    gridTemplateColumns: [`minmax(${TIME_COLUMN_WIDTH.min}px, ${timePreferredWidth}px)`, ...tracks].join(' '),
    minWidth: `${minimumContentWidth + 32}px`,
  };
}

export function levelToneClass(level: string): string {
  if (level === 'ERROR') return 'bg-red/12 text-red';
  if (level === 'WARN') return 'bg-orange/12 text-orange-soft';
  if (level === 'DEBUG' || level === 'TRACE') return 'bg-bg-3 text-tx-2';
  return 'bg-blue/10 text-blue-soft';
}

function LogResultValue({ log, field }: { log: LogEntry; field: string }) {
  const rawValue = logResultRawValue(log, field);
  const value = displayLogValue(rawValue);
  if (LEVEL_FIELDS.has(field) && value) {
    const level = value.toUpperCase();
    return (
      <span className={`type-micro w-fit rounded px-1.5 py-0.5 font-mono font-semibold ${levelToneClass(level)}`}>
        {level}
      </span>
    );
  }
  return (
    <span className="block truncate" title={value}>
      {value || '—'}
    </span>
  );
}

interface LogListResultsProps {
  rows: LogEntry[];
  fields: string[];
  timezone: string;
  startIndex: number;
  selectedIndex: number | null;
  density: LogResultDensity;
  onSelect: (index: number) => void;
}

export function LogListResults({
  rows,
  fields,
  timezone,
  startIndex,
  selectedIndex,
  density,
  onSelect,
}: LogListResultsProps) {
  const { t } = useTranslation('logs');
  const columns = React.useMemo(() => visibleResultFields(fields), [fields]);
  const timeColumnLabel = t('explore.table.time_column', { timezone });
  const rowHeight = logResultRowHeight(density);
  const gridStyle = React.useMemo(
    () => resultGridStyle(columns, rows, timeColumnLabel),
    [columns, rows, timeColumnLabel],
  );
  return (
    <div className="h-full min-h-0 overflow-auto">
      <div
        data-log-result-columns="logs"
        style={gridStyle}
        className={`sticky top-0 z-10 grid gap-3 border-b border-bd-0 bg-bg-1 px-4 py-2 ${uiTableHeaderClass}`}
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
        const selected = index === selectedIndex;
        return (
          <div
            key={index}
            data-log-result-row="logs"
            role="button"
            tabIndex={0}
            aria-label={t('explore.table.row_aria', { index: index + 1 })}
            onClick={() => onSelect(index)}
            onKeyDown={(event) => {
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                onSelect(index);
              }
            }}
            style={{ ...gridStyle, minHeight: rowHeight }}
            className={`grid cursor-pointer items-center gap-3 border-b border-bd-0 px-4 text-left font-sans text-xs hover:bg-bg-2 focus:bg-bg-2 ${
              selected ? 'bg-indigo-dim text-indigo-soft' : ''
            }`}
          >
            <span
              data-log-field="_timestamp"
              className="type-micro min-w-0 truncate whitespace-nowrap font-mono text-tx-2"
              title={log.ts}
            >
              {log.ts}
            </span>
            {columns.map((field) => (
              <span key={field} data-log-field={field} className="min-w-0 text-tx-1">
                <LogResultValue log={log} field={field} />
              </span>
            ))}
          </div>
        );
      })}
    </div>
  );
}
