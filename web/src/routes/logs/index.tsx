import { useQuery } from '@tanstack/react-query';
import {
  ArrowLeft,
  ArrowRight,
  Braces,
  ChevronRight,
  Clipboard,
  Columns3,
  Download,
  Eye,
  EyeOff,
  FileText,
  Filter,
  List,
  Minus,
  MoreVertical,
  Play,
  Plus,
  RefreshCw,
  Rows3,
  Search,
  SlidersHorizontal,
  Table2,
  X,
  type LucideIcon,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useSearchParams } from 'react-router-dom';

import * as fieldMaskingApi from '@/api/fieldMasking';
import * as queryApi from '@/api/query';
import * as streamsApi from '@/api/streams';
import type { LogListResponse } from '@/api/web';
import {
  type DateFormat,
  formatMicros,
  type TimeFormat,
  useTimeFormatter,
} from '@/lib/time';
import { useCursorPagination } from '@/pagination/useCursorPagination';
import { ChromeButton, Pill, TimeRangeChip, uiTableHeaderClass } from '@/shell/chrome';
import type { CodeCompletionItem } from '@/shell/codeEditor/types';
import { CollapsibleSidePanel, SidePanelSection } from '@/shell/CollapsibleSidePanel';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { CursorPagination } from '@/shell/CursorPagination';
import { PageHeader } from '@/shell/PageHeader';
import { QueryEditorFrame } from '@/shell/query/EditorFrame';
import { QueryRecommendations } from '@/shell/query/Recommendations';
import { QueryState } from '@/shell/query/State';
import { QuerySyntaxHelp } from '@/shell/query/SyntaxHelp';
import { useSqlFunctionCompletions } from '@/shell/query/useSqlFunctionCompletions';
import { QueryToolbarButton, QueryToolbarGroup, QueryWorkbench } from '@/shell/query/Workbench';
import { ResultPagination } from '@/shell/ResultPagination';
import { detectSignalTypeForLabel, SignalReference } from '@/shell/SignalReference';
import { TimezoneSelect } from '@/shell/TimezoneSelect';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import { toast } from '@/shell/ui/sonner';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/shell/ui/tooltip';
import { useAuthStore } from '@/stores/auth';
import { useFiltersStore, type GlobalFilter } from '@/stores/useFiltersStore';
import { resolveWindow, useTimeStore } from '@/stores/useTimeStore';
import { formatTimeWindowLabel } from '@/time/TimePicker';
import type { QueryResult } from '@/types/query';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import {
  LogColumnMenu,
  reorderVisibleLogFields,
  type LogColumnDropPosition,
} from './ColumnMenu';
import { runLogCursorQuery } from './cursorQuery';
import {
  appendLogFieldClause,
  appendLogFieldValueClause,
  buildLogFieldQuerySql,
  deriveLogFields,
  escapeSqlIdentifier,
  escapeSqlString,
  isLogFieldFilterable,
  type LogFieldDef,
} from './fieldQueryModel';
import { HistogramToggle } from './HistogramToggle';
import { levelToneClass, LogListResults } from './ResultViews';
import {
  defaultLogTableFields,
  displayLogValue,
  logLevelLabel,
  logSourceLabel,
  primaryLogMessage,
  recordsToCsv,
  recordsToLogText,
  topLogFieldValues,
  type LogResultDensity,
  type LogEntry,
  type LogLevel,
  type LogTopValue,
} from './viewModel';

interface LogHistogramRange {
  start: number;
  end: number;
}

interface LogHistogramBucket {
  ok: number;
  err: number;
  startMicros: number;
  endMicros: number;
}

const DEFAULT_LOG_LIMIT = 200;
const LOG_PAGE_SIZE_OPTIONS = [25, 50, 100, 200];
const LOG_CURSOR_PAGE_SIZE_OPTIONS = [20, 50, 100];

type LogQueryMode = 'fields' | 'sql';

interface LogQueryTemplate {
  id: string;
  labelKey: string;
  descriptionKey: string;
  statement: string;
}

const TYPE_GLYPH: Record<LogFieldDef['type'], string> = {
  string: 'T',
  number: '#',
  timestamp: 'T',
  date: 'D',
  array: '[]',
  boolean: 'B',
  object: '{}',
};
const TYPE_COLOR: Record<LogFieldDef['type'], string> = {
  // Stays in sync with the JSON-tree value palette below (see `valColor`
  // around line 1224): the same data type reads as the same color whether
  // it appears as a field-type swatch on the left or as a rendered value
  // in the row. Brief: `key tx-2，string green，number orange，level keyword indigo`.
  string: 'text-green-soft',
  number: 'text-orange-soft',
  timestamp: 'text-green-soft',
  date: 'text-green-soft',
  array: 'text-purple-soft',
  boolean: 'text-blue-soft',
  object: 'text-purple-soft',
};

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
}

function appendLogQueryExpression(current: string, expression: string): string {
  const trimmed = current.trim();
  if (!trimmed) return expression;
  if (trimmed.includes(expression)) return trimmed;
  if (/\bAND\s*$/i.test(trimmed)) return `${trimmed} ${expression}`;
  return `${trimmed} AND ${expression}`;
}

function logQueryPlaceholder(mode: LogQueryMode, stream: string): string {
  if (mode === 'sql') return stream ? defaultLogQuery(stream) : 'Select a stream to start querying logs.';
  return 'trace_id = "..." / service_name contains "checkout"';
}

function defaultLogQuery(stream: string): string {
  return `SELECT * FROM "${escapeSqlIdentifier(stream)}"\nORDER BY _timestamp DESC\nLIMIT ${DEFAULT_LOG_LIMIT}`;
}

function logExecutionKey(mode: LogQueryMode, stream: string, sql: string, fields: string): string {
  const selectedStream = stream.trim();
  if (!selectedStream) return '';
  if (mode === 'sql') return `sql:${selectedStream}:${sql.trim() || defaultLogQuery(selectedStream)}`;
  return `fields:${selectedStream}:${fields.trim()}`;
}

function quotedCompletion(value: string): string {
  return `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

function logQueryCompletions(
  fields: LogFieldDef[],
  rows: LogEntry[],
  maskedFields: ReadonlySet<string> | null,
): CodeCompletionItem[] {
  const fieldNames = new Set([
    'level',
    'message',
    'body',
    'service',
    'service_name',
    'trace_id',
    'span_id',
    'status',
    'status_code',
    'duration',
    'duration_ms',
    ...fields.map((field) => field.name),
  ]);
  const values = new Map<string, Set<string>>([
    ['level', new Set(['INFO', 'WARN', 'ERROR', 'DEBUG', 'TRACE'])],
    ['service', new Set(['checkout', 'api', 'web'])],
    ['service_name', new Set(['checkout', 'api', 'web'])],
  ]);
  // Until masking metadata is known, expose only the static, non-sensitive
  // fallbacks above. Observed log values are added only for confirmed
  // unmasked fields so autocomplete cannot become a masking side channel.
  if (maskedFields !== null) {
    for (const row of rows.slice(0, 200)) {
      if (!maskedFields.has('level')) values.get('level')?.add(row.level);
      for (const [key, value] of Object.entries(row.raw)) {
        if (!fieldNames.has(key) || maskedFields.has(key.toLowerCase())) continue;
        const bucket = values.get(key) ?? new Set<string>();
        values.set(key, bucket);
        if (bucket.size >= 50) continue;
        if (typeof value === 'string' && value.length > 0 && value.length <= 80) {
          bucket.add(value);
        }
        if (typeof value === 'number' || typeof value === 'boolean') {
          bucket.add(String(value));
        }
      }
    }
  }
  return [
    ...Array.from(fieldNames).sort().map((label) => ({ label, kind: 'field' as const, detail: 'log field' })),
    { label: '=', insertText: '= ', kind: 'operator', detail: 'operator' },
    { label: '!=', insertText: '!= ', kind: 'operator', detail: 'operator' },
    { label: '>=', insertText: '>= ', kind: 'operator', detail: 'operator' },
    { label: '<=', insertText: '<= ', kind: 'operator', detail: 'operator' },
    { label: '>', insertText: '> ', kind: 'operator', detail: 'operator' },
    { label: '<', insertText: '< ', kind: 'operator', detail: 'operator' },
    { label: 'contains', insertText: 'contains ', kind: 'operator', detail: 'operator' },
    { label: 'AND', insertText: 'AND ', kind: 'operator', detail: 'operator' },
    { label: 'OR', insertText: 'OR ', kind: 'operator', detail: 'operator' },
    ...Array.from(values.entries()).flatMap(([field, fieldValues]) => (
      Array.from(fieldValues).sort().map((value) => {
        const quoted = quotedCompletion(value);
        return {
          label: quoted,
          insertText: quoted,
          kind: 'value' as const,
          detail: `${field} value`,
          field,
          value,
        };
      })
    )),
  ];
}

function hasSqlFieldFilter(statement: string, field: string): boolean {
  const quotedField = `"${escapeSqlIdentifier(field)}"`;
  const escapedQuoted = escapeRegExp(quotedField);
  const escapedBare = escapeRegExp(field);
  const pattern = new RegExp(`(?:^|\\s|\\()(${escapedQuoted}|${escapedBare})\\s*(?:>=|<=|=|!=|<>|>|<|LIKE\\b|IN\\s*\\(|IS\\s+(?:NOT\\s+)?NULL)`, 'i');
  return pattern.test(statement);
}

function appendSqlFieldFilter(statement: string, stream: string, field: LogFieldDef): string {
  const base = statement.trim() || defaultLogQuery(stream);
  if (hasSqlFieldFilter(base, field.name)) return statement;
  const identifier = `"${escapeSqlIdentifier(field.name)}"`;
  const clause = field.dataType === 'json'
    ? `CAST(${identifier} AS VARCHAR) LIKE '%"key"%'`
    : field.dataType === 'bool'
      ? `${identifier} = TRUE`
      : field.dataType === 'timestamp'
        ? `CAST(${identifier} AS BIGINT) >= 0`
        : ['int64', 'float64'].includes(field.dataType)
        ? `${identifier} >= 0`
        : `${identifier} = ''`;
  const boundary = base.search(/\s+(ORDER\s+BY|LIMIT)\b/i);
  if (boundary >= 0) {
    const before = base.slice(0, boundary).trimEnd();
    const after = base.slice(boundary);
    const joiner = /\bWHERE\b/i.test(before) ? `\n  AND ${clause}` : `\nWHERE ${clause}`;
    return `${before}${joiner}${after}`;
  }
  return /\bWHERE\b/i.test(base) ? `${base}\n  AND ${clause}` : `${base}\nWHERE ${clause}`;
}

function appendSqlFieldValueFilter(
  statement: string,
  stream: string,
  field: string,
  value: unknown,
  mode: 'include' | 'exclude',
): string {
  const base = statement.trim() || defaultLogQuery(stream);
  const sqlValue = typeof value === 'number' && Number.isFinite(value)
    ? String(value)
    : typeof value === 'boolean'
      ? String(value).toUpperCase()
      : `'${escapeSqlString(displayLogValue(value))}'`;
  const clause = `"${escapeSqlIdentifier(field)}" ${mode === 'exclude' ? '!=' : '='} ${sqlValue}`;
  const boundary = base.search(/\s+(ORDER\s+BY|LIMIT)\b/i);
  const before = boundary >= 0 ? base.slice(0, boundary).trimEnd() : base;
  const after = boundary >= 0 ? base.slice(boundary) : '';
  const joiner = /\bWHERE\b/i.test(before) ? `\n  AND ${clause}` : `\nWHERE ${clause}`;
  return `${before}${joiner}${after}`;
}

/**
 * Append pinned global filters (`useFiltersStore`) to the SQL about to run, as
 * `"key" = 'value'` clauses inserted before any ORDER BY / LIMIT. Filters whose
 * field the query already constrains are skipped (no conflict / duplication),
 * and an empty filter set is a no-op — so behavior is unchanged when nothing is
 * pinned.
 */
function appendGlobalFilters(statement: string, stream: string, filters: GlobalFilter[]): string {
  if (filters.length === 0) return statement;
  const base = statement.trim() || defaultLogQuery(stream);
  const clauses = filters
    .filter((f) => f.key && f.value && !hasSqlFieldFilter(base, f.key))
    .map((f) => `"${escapeSqlIdentifier(f.key)}" ${f.operator === '!=' ? '!=' : '='} '${f.value.replace(/'/g, "''")}'`);
  if (clauses.length === 0) return base;
  const boundary = base.search(/\s+(ORDER\s+BY|LIMIT)\b/i);
  const body = boundary >= 0 ? base.slice(0, boundary).trimEnd() : base;
  const tail = boundary >= 0 ? base.slice(boundary) : '';
  let hasWhere = /\bWHERE\b/i.test(body);
  let result = body;
  for (const clause of clauses) {
    result += hasWhere ? `\n  AND ${clause}` : `\nWHERE ${clause}`;
    hasWhere = true;
  }
  return `${result}${tail}`;
}

function isPresent(value: unknown): boolean {
  return value !== null && value !== undefined && value !== '';
}

function rowToRecord(row: unknown[], columns: string[]): Record<string, unknown> {
  return columns.reduce<Record<string, unknown>>((acc, column, index) => {
    acc[column] = row[index];
    return acc;
  }, {});
}

function formatTimestamp(
  value: unknown,
  tz: string,
  format: TimeFormat,
  dateFormat: DateFormat,
): string {
  if (!isPresent(value)) return '—';
  const micros = timestampToMicros(value);
  return micros == null
    ? String(value)
    : formatMicros(micros, tz, format, true, dateFormat);
}

function timestampToMicros(value: unknown): number | null {
  const numeric = typeof value === 'number'
    ? value
    : typeof value === 'string' && /^-?\d+(?:\.\d+)?$/.test(value.trim())
      ? Number(value)
      : null;
  if (numeric != null && Number.isFinite(numeric)) {
    const absolute = Math.abs(numeric);
    if (absolute > 1e17) return Math.floor(numeric / 1_000);
    if (absolute > 1e14) return Math.floor(numeric);
    if (absolute > 1e11) return Math.floor(numeric * 1_000);
    return Math.floor(numeric * 1_000_000);
  }
  if (typeof value !== 'string') return null;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed * 1_000;
}

function compactRecord(record: Record<string, unknown>): string {
  try {
    return JSON.stringify(record) ?? '';
  } catch {
    return String(record);
  }
}

const COMMON_LOG_FIELD_ORDER = [
  '_timestamp',
  'timestamp',
  'service',
  'service_name',
  'level',
  'message',
  'body',
  'trace_id',
] as const;
const COMMON_LOG_FIELDS = new Set<string>(COMMON_LOG_FIELD_ORDER);

function formatLogFieldValue(value: unknown): string {
  if (!isPresent(value)) return '';
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean') return String(value);
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
}

function stringLabelsFromRecord(record: Record<string, unknown>): Record<string, string> {
  const labels: Record<string, string> = {};
  for (const [key, value] of Object.entries(record)) {
    if (typeof value === 'string' && value) labels[key] = value;
    else if (typeof value === 'number' || typeof value === 'boolean') labels[key] = String(value);
  }
  return labels;
}

interface FieldJumpContext {
  record: Record<string, unknown>;
  field: string;
  value: unknown;
}

interface FieldJumpTarget {
  to: string;
  /** i18n key — the consumer calls `t(titleKey)` so the locale is right. */
  titleKey: string;
}

interface FieldJumpConfig {
  match: (field: string) => boolean;
  resolve: (context: FieldJumpContext) => FieldJumpTarget | null;
}

const TRACE_ID_FIELD_ALIASES = ['trace_id', 'traceid', 'trace.id'];
const SPAN_ID_FIELD_ALIASES = ['span_id', 'spanid', 'span.id'];

function leafFieldName(field: string): string {
  const parts = field.toLowerCase().replace(/\[(\d+)\]/g, '.$1').split('.').filter(Boolean);
  return parts.length > 0 ? parts[parts.length - 1]! : field.toLowerCase();
}

function matchesFieldAlias(field: string, aliases: string[]): boolean {
  const lower = field.toLowerCase();
  const leaf = leafFieldName(field);
  return aliases.some((alias) => {
    const normalized = alias.toLowerCase();
    return lower === normalized || leaf === normalized || lower.endsWith(`.${normalized}`);
  });
}

function readRecordPath(record: Record<string, unknown>, path: string): unknown {
  if (Object.prototype.hasOwnProperty.call(record, path)) return record[path];
  let current: unknown = record;
  for (const part of path.split('.')) {
    if (!current || typeof current !== 'object' || !Object.prototype.hasOwnProperty.call(current, part)) {
      return undefined;
    }
    current = (current as Record<string, unknown>)[part];
  }
  return current;
}

function findRecordValue(record: Record<string, unknown>, aliases: string[]): unknown {
  for (const alias of aliases) {
    const value = readRecordPath(record, alias);
    if (isPresent(value)) return value;
  }
  for (const [key, value] of Object.entries(record)) {
    if (matchesFieldAlias(key, aliases) && isPresent(value)) return value;
  }
  return undefined;
}

// Jump configs carry an i18n key, not a translated title — the consumer
// component owns the `t()` call so the tooltip follows the user's locale.
const LOG_FIELD_JUMP_CONFIG: FieldJumpConfig[] = [
  {
    match: (field) => matchesFieldAlias(field, TRACE_ID_FIELD_ALIASES),
    resolve: ({ value }) => {
      if (!isPresent(value)) return null;
      return {
        to: `/traces/${encodeURIComponent(String(value))}`,
        titleKey: 'explore.jump.open_trace',
      };
    },
  },
  {
    match: (field) => matchesFieldAlias(field, SPAN_ID_FIELD_ALIASES),
    resolve: ({ record, value }) => {
      const traceId = findRecordValue(record, TRACE_ID_FIELD_ALIASES);
      if (!isPresent(traceId) || !isPresent(value)) return null;
      return {
        to: `/traces/${encodeURIComponent(String(traceId))}?spanId=${encodeURIComponent(String(value))}`,
        titleKey: 'explore.jump.open_span',
      };
    },
  },
];

function resolveLogFieldJump(record: Record<string, unknown>, field: string, value: unknown): FieldJumpTarget | null {
  for (const config of LOG_FIELD_JUMP_CONFIG) {
    if (!config.match(field)) continue;
    const target = config.resolve({ record, field, value });
    if (target) return target;
  }
  return null;
}

function logRecordJson(record: Record<string, unknown>): string {
  return JSON.stringify(record, null, 2);
}

async function copyTextToClipboard(textToCopy: string): Promise<void> {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(textToCopy);
    return;
  }
  const textarea = document.createElement('textarea');
  textarea.value = textToCopy;
  textarea.setAttribute('readonly', 'true');
  textarea.style.position = 'fixed';
  textarea.style.top = '-1000px';
  document.body.appendChild(textarea);
  textarea.select();
  document.execCommand('copy');
  textarea.remove();
}

function downloadJsonFile(filename: string, value: unknown): void {
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

function downloadTextFile(filename: string, value: string, type: string): void {
  const blob = new Blob([value], { type });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

/**
 * Map a `/query` row tuple to the display LogEntry shape using a column
 * index map. Missing display columns degrade only in presentation fields;
 * the raw row is kept intact for the source line and detail drawer.
 */
function rowToLog(
  row: unknown[],
  columns: string[],
  idx: Record<string, number>,
  tz: string,
  format: TimeFormat,
  dateFormat: DateFormat,
): LogEntry {
  const get = (k: string): unknown => (idx[k] !== undefined ? row[idx[k]!] : undefined);
  const level = String(get('level') ?? get('body_level') ?? 'INFO').toUpperCase() as LogLevel;
  return {
    ts: formatTimestamp(
      get('_timestamp') ?? get('timestamp') ?? get('time'),
      tz,
      format,
      dateFormat,
    ),
    level,
    raw: rowToRecord(row, columns),
  };
}

function buildHisto(rows: LogEntry[], range: LogHistogramRange | null): LogHistogramBucket[] {
  const buckets = 80;
  const timestamps = rows
    .map((row) => timestampToMicros(row.raw._timestamp ?? row.raw.timestamp ?? row.raw.time))
    .filter((value): value is number => value != null);
  const fallbackStart = timestamps.length > 0 ? Math.min(...timestamps) : Date.now() * 1_000;
  const fallbackEnd = timestamps.length > 0 ? Math.max(...timestamps) + 1 : fallbackStart + 1;
  const start = range && range.end > range.start ? range.start : fallbackStart;
  const end = range && range.end > range.start ? range.end : fallbackEnd;
  const width = (end - start) / buckets;
  const arr = Array.from({ length: buckets }, (_, index) => ({
    ok: 0,
    err: 0,
    startMicros: Math.floor(start + width * index),
    endMicros: index === buckets - 1 ? end : Math.floor(start + width * (index + 1)),
  }));
  rows.forEach((l, i) => {
    const timestamp = timestampToMicros(l.raw._timestamp ?? l.raw.timestamp ?? l.raw.time);
    const fallbackBucket = Math.floor((i / Math.max(rows.length, 1)) * buckets);
    const b = timestamp == null
      ? fallbackBucket
      : Math.max(0, Math.min(buckets - 1, Math.floor(((timestamp - start) / (end - start)) * buckets)));
    if (l.level === 'ERROR') arr[b]!.err++;
    else arr[b]!.ok++;
  });
  return arr;
}

const MODE_OPTIONS = [
  { id: 'search', ariaKey: 'explore.toolbar.mode_search_aria' },
  { id: 'patterns', ariaKey: 'explore.toolbar.mode_patterns_aria' },
] as const;

const LOG_QUERY_MODES: Array<{ id: LogQueryMode; labelKey: string }> = [
  { id: 'fields', labelKey: 'explore.query_modes.fields' },
  { id: 'sql', labelKey: 'explore.query_modes.sql' },
];

function requestedLogFieldQueryFromParams(params: URLSearchParams): string {
  const direct = params.get('q') ?? params.get('query') ?? '';
  if (direct.trim()) return direct.trim();
  const clauses: string[] = [];
  const traceId = params.get('trace_id') ?? params.get('traceId');
  const spanId = params.get('span_id') ?? params.get('spanId');
  const service = params.get('service') ?? params.get('service_name');
  const host = params.get('host') ?? params.get('hostname');
  const path = params.get('path') ?? params.get('route');
  const method = params.get('method');
  const status = params.get('status_code') ?? params.get('status');
  if (traceId) clauses.push(logParamClause('trace_id', traceId));
  if (spanId) clauses.push(logParamClause('span_id', spanId));
  if (service) clauses.push(logParamClause('service', service));
  if (host) clauses.push(logParamClause('host', host));
  if (path) clauses.push(logParamClause('path', path));
  if (method) clauses.push(logParamClause('method', method));
  if (status) clauses.push(logParamClause('status_code', status));
  return clauses.join(' AND ');
}

function logParamClause(field: string, value: string): string {
  return `${field}='${value.replace(/\\/g, '\\\\').replace(/'/g, "\\'")}'`;
}

export function Logs() {
  const { t } = useTranslation('logs');
  const { t: tCommon } = useTranslation('common');
  const [tzOverride, setTzOverride] = React.useState('');
  const fmt = useTimeFormatter({ timezone: tzOverride || undefined });
  const [searchParams] = useSearchParams();
  const requestedStream = searchParams.get('stream') ?? '';
  const requestedFieldQuery = requestedLogFieldQueryFromParams(searchParams);
  // `?sql=<statement>` seeds the raw-SQL editor (used by Saved Views "Open" for
  // SQL-language views, which can't round-trip through the `?q=` field DSL).
  const requestedSql = searchParams.get('sql')?.trim() ?? '';
  const [mode, setMode] = React.useState<'search' | 'patterns'>('search');
  const [queryMode, setQueryMode] = React.useState<LogQueryMode>(requestedSql ? 'sql' : 'fields');
  const [stream, setStream] = React.useState(requestedStream);
  const [query, setQuery] = React.useState(() =>
    requestedSql || (requestedStream ? defaultLogQuery(requestedStream) : ''),
  );
  const [fieldQuery, setFieldQuery] = React.useState(requestedFieldQuery);
  const [showHistogram, setShowHistogram] = React.useState(true);
  const [fieldPanelCollapsed, setFieldPanelCollapsed] = React.useState(false);
  const [queryEditorCollapsed, setQueryEditorCollapsed] = React.useState(false);
  const [fieldFilter, setFieldFilter] = React.useState('');
  const [visibleLogFields, setVisibleLogFields] = React.useState<string[]>([]);
  const [resultDensity, setResultDensity] = React.useState<LogResultDensity>('compact');
  const [expandedField, setExpandedField] = React.useState<string | null>(null);
  const [resultPage, setResultPage] = React.useState(1);
  const [resultPageSize, setResultPageSize] = React.useState(50);
  const [selectedRow, setSelectedRow] = React.useState<number | null>(null);
  const [queryResult, setQueryResult] = React.useState<QueryResult | undefined>();
  const [logCursorPage, setLogCursorPage] = React.useState<LogListResponse | null>(null);
  // The actual statement + time-range that produced `queryResult`, captured at
  // run time so the optimization tips reflect what was executed (not the live editor).
  const [executedStatement, setExecutedStatement] = React.useState('');
  const [executedRangeSecs, setExecutedRangeSecs] = React.useState<number | null>(null);
  const [executedTimeRange, setExecutedTimeRange] = React.useState<LogHistogramRange | null>(null);
  const [queryError, setQueryError] = React.useState<unknown>(null);
  const [queryPending, setQueryPending] = React.useState(false);
  const [lastExecutedQueryKey, setLastExecutedQueryKey] = React.useState<string | null>(null);
  const [functionPanelOpen, setFunctionPanelOpen] = React.useState(false);
  const initialRunStreamRef = React.useRef<string | null>(null);
  const appliedFieldQueryRef = React.useRef(requestedFieldQuery);
  const appliedSqlRef = React.useRef(requestedSql);

  const orgId = useAuthStore((s) => s.ctx?.org_id ?? '');
  const timeWindow = useTimeStore((s) => s.window);
  const setTimeWindow = useTimeStore((s) => s.setWindow);
  const globalFilters = useFiltersStore((s) => s.filters);
  const previousTimeWindowRef = React.useRef(timeWindow);
  const logCursorContextKey = React.useMemo(
    () =>
      JSON.stringify({
        orgId,
        stream,
        fieldQuery,
        globalFilters,
        timeWindow,
      }),
    [fieldQuery, globalFilters, orgId, stream, timeWindow],
  );
  const {
    pageSize: logPageSize,
    reset: resetLogCursor,
    goPrevious: goToPreviousLogPage,
    goNext: goToNextLogPage,
    setPageSize: setLogPageSize,
  } = useCursorPagination({
    contextKey: logCursorContextKey,
    defaultPageSize: 20,
  });

  const streamsQuery = useQuery({
    queryKey: ['streams', 'logs-selector'],
    queryFn: () => streamsApi.list(500),
  });
  // Logs always queries with `stream_type: 'logs'`, so the selector must only
  // offer logs streams. Otherwise the auto-select picks `streams[0]` — which
  // after on-demand creation can be a traces/metrics stream (e.g. a `default`
  // traces stream from a test trace) — and the query 404s with
  // `stream not found: default`.
  const streams = React.useMemo(
    () => (streamsQuery.data ?? []).filter((s) => s.type === 'logs' && streamsApi.isQueryable(s)),
    [streamsQuery.data],
  );

  const setAutoStream = React.useCallback((nextStream: string, options?: { preserveFieldQuery?: boolean }) => {
    const nextQuery = defaultLogQuery(nextStream);
    setStream(nextStream);
    setQuery(nextQuery);
    if (!options?.preserveFieldQuery) setFieldQuery('');
    setSelectedRow(null);
    setResultPage(1);
    setQueryResult(undefined);
    setLogCursorPage(null);
    setQueryError(null);
    setQueryEditorCollapsed(false);
    setExpandedField(null);
  }, []);

  const executeQuery = React.useCallback(async (
    options: { cursor?: string; pageSize?: number } = {},
  ) => {
    const selectedStream = stream.trim();
    if (!selectedStream) {
      setQueryError(new Error('Select a stream before querying.'));
      return;
    }
    setQueryPending(true);
    setQueryError(null);
    try {
      const resolvedWindow = resolveWindow(timeWindow);
      const queryTimeRange = options.cursor && executedTimeRange
        ? executedTimeRange
        : {
            start: resolvedWindow.from.getTime() * 1000,
            end: resolvedWindow.to.getTime() * 1000,
          };
      const baseStatement = queryMode === 'sql'
        ? (query.trim() || defaultLogQuery(selectedStream))
        : buildLogFieldQuerySql(selectedStream, fieldQuery, options.pageSize ?? logPageSize);
      const statement = appendGlobalFilters(baseStatement, selectedStream, globalFilters);
      const queryKey = logExecutionKey(queryMode, selectedStream, query, fieldQuery);
      let result: QueryResult;
      if (queryMode === 'fields') {
        const { page, result: cursorResult } = await runLogCursorQuery({
          stream: selectedStream,
          statement: fieldQuery,
          globalFilters,
          timeRange: queryTimeRange,
          pageSize: options.pageSize ?? logPageSize,
          cursor: options.cursor,
        });
        if (!options.cursor) resetLogCursor();
        setLogCursorPage(page);
        result = cursorResult;
      } else {
        resetLogCursor();
        setLogCursorPage(null);
        result = await queryApi.runQuery({
          org_id: orgId,
          language: 'sql',
          statement,
          time_range: queryTimeRange,
          stream: { name: selectedStream, stream_type: 'logs' },
          limit: DEFAULT_LOG_LIMIT,
        });
      }
      setQueryResult(result);
      setExecutedStatement(statement);
      setExecutedTimeRange(queryTimeRange);
      setExecutedRangeSecs(
        Math.max(0, Math.round((queryTimeRange.end - queryTimeRange.start) / 1_000_000)),
      );
      setSelectedRow(null);
      setResultPage(1);
      setLastExecutedQueryKey(queryKey);
      setFunctionPanelOpen(false);
    } catch (err) {
      setQueryError(err);
    } finally {
      setQueryPending(false);
    }
  }, [
    executedTimeRange,
    fieldQuery,
    globalFilters,
    logPageSize,
    orgId,
    query,
    queryMode,
    resetLogCursor,
    stream,
    timeWindow,
  ]);

  React.useEffect(() => {
    if (!requestedStream || requestedStream === stream) return;
    setAutoStream(requestedStream, { preserveFieldQuery: Boolean(requestedFieldQuery) });
  }, [requestedFieldQuery, requestedStream, setAutoStream, stream]);

  React.useEffect(() => {
    if (stream || streams.length === 0) return;
    // Streams are queried by name (`stream: { name }`), so the default
    // selection must be the stream *name* — using `.id` here makes the backend
    // reject with `stream not found: <id>`.
    setAutoStream(streams[0]!.name, { preserveFieldQuery: Boolean(requestedFieldQuery) });
  }, [requestedFieldQuery, setAutoStream, stream, streams]);

  React.useEffect(() => {
    if (requestedFieldQuery === appliedFieldQueryRef.current) return;
    appliedFieldQueryRef.current = requestedFieldQuery;
    setQueryMode('fields');
    setFieldQuery(requestedFieldQuery);
    setSelectedRow(null);
    setResultPage(1);
    setQueryResult(undefined);
    setLogCursorPage(null);
    setQueryError(null);
    setQueryEditorCollapsed(false);
    initialRunStreamRef.current = null;
  }, [requestedFieldQuery]);

  // Mirror of the field-query effect for the raw-SQL seed (`?sql=`): drop into
  // SQL mode with the statement verbatim and let the auto-run effect fire.
  React.useEffect(() => {
    if (!requestedSql || requestedSql === appliedSqlRef.current) return;
    appliedSqlRef.current = requestedSql;
    setQueryMode('sql');
    setQuery(requestedSql);
    setSelectedRow(null);
    setResultPage(1);
    setQueryResult(undefined);
    setLogCursorPage(null);
    setQueryError(null);
    setQueryEditorCollapsed(false);
    initialRunStreamRef.current = null;
  }, [requestedSql]);

  React.useEffect(() => {
    const runKey = `${orgId}:${stream}`;
    if (!orgId || !stream || (queryMode === 'sql' && !query.trim()) || initialRunStreamRef.current === runKey) return;
    initialRunStreamRef.current = runKey;
    void executeQuery();
    // Runs once for the active org + stream. Re-runs are explicit through the
    // query / refresh controls.
  }, [executeQuery, orgId, query, queryMode, stream]);

  const rows = React.useMemo<LogEntry[]>(() => {
    if (!queryResult) return [];
    const idx: Record<string, number> = {};
    queryResult.columns.forEach((c, i) => {
      idx[c] = i;
    });
    return queryResult.rows.map((row) =>
      rowToLog(
        row,
        queryResult.columns,
        idx,
        fmt.tz,
        fmt.format,
        fmt.dateFormat,
      ),
    );
  }, [queryResult, fmt.tz, fmt.format, fmt.dateFormat]);
  const rawRecords = React.useMemo(() => rows.map((row) => row.raw), [rows]);

  const cursorResults = queryMode === 'fields';
  const resultPageCount = cursorResults
    ? 1
    : Math.max(1, Math.ceil(rows.length / resultPageSize));
  const activeResultPage = Math.min(resultPage, resultPageCount);
  const resultPageStart = cursorResults ? 0 : (activeResultPage - 1) * resultPageSize;
  const pagedRows = React.useMemo(
    () => cursorResults ? rows : rows.slice(resultPageStart, resultPageStart + resultPageSize),
    [cursorResults, resultPageSize, resultPageStart, rows],
  );

  React.useEffect(() => {
    setResultPage((current) => Math.min(current, resultPageCount));
  }, [resultPageCount]);

  const changeResultPage = React.useCallback((nextPage: number) => {
    setResultPage(Math.min(Math.max(1, nextPage), resultPageCount));
    setSelectedRow(null);
  }, [resultPageCount]);

  const changeResultPageSize = React.useCallback((nextPageSize: number) => {
    setResultPageSize(nextPageSize);
    setResultPage(1);
    setSelectedRow(null);
  }, []);

  const changeLogPage = React.useCallback((direction: 'previous' | 'next') => {
    const token = direction === 'previous'
      ? logCursorPage?.previous_cursor
      : logCursorPage?.next_cursor;
    if (!token) return;
    if (direction === 'previous') goToPreviousLogPage(logCursorPage);
    else goToNextLogPage(logCursorPage);
    setSelectedRow(null);
    void executeQuery({ cursor: token });
  }, [executeQuery, goToNextLogPage, goToPreviousLogPage, logCursorPage]);

  const changeLogPageSize = React.useCallback((nextPageSize: number) => {
    setLogPageSize(nextPageSize);
    setSelectedRow(null);
    void executeQuery({ pageSize: nextPageSize });
  }, [executeQuery, setLogPageSize]);

  const histo = React.useMemo(() => buildHisto(rows, executedTimeRange), [executedTimeRange, rows]);
  const selectHistogramRange = React.useCallback(
    ({ from, to }: { from: number; to: number }) =>
      setTimeWindow({
        mode: 'absolute',
        from: new Date(from / 1000).toISOString(),
        to: new Date(to / 1000).toISOString(),
      }),
    [setTimeWindow],
  );

  const selectedStream = React.useMemo(
    () => streams.find((candidate) => candidate.name === stream && candidate.stream_type === 'logs'),
    [stream, streams],
  );
  const selectedStreamFields = React.useMemo(
    () => selectedStream?.schema.fields ?? [],
    [selectedStream],
  );
  const fieldMaskingQuery = useQuery({
    queryKey: ['field-masking-effective', selectedStream?.id],
    queryFn: () => fieldMaskingApi.effectiveForStream(selectedStream?.id ?? ''),
    enabled: Boolean(selectedStream?.id),
  });
  const maskedFields = React.useMemo(
    () => fieldMaskingQuery.data
      ? new Set(
          fieldMaskingQuery.data.fields
            .filter((field) => field.masked)
            .map((field) => field.field.toLowerCase()),
        )
      : null,
    [fieldMaskingQuery.data],
  );
  const fields = React.useMemo(
    () => deriveLogFields(selectedStreamFields, queryResult),
    [queryResult, selectedStreamFields],
  );
  const filteredFields = React.useMemo(() => {
    const needle = fieldFilter.trim().toLowerCase();
    return fields.filter((field) => field.name.toLowerCase().includes(needle));
  }, [fieldFilter, fields]);
  const displayedFields = React.useMemo(
    () => visibleLogFields
      .map((name) => filteredFields.find((field) => field.name === name))
      .filter((field): field is LogFieldDef => field !== undefined),
    [filteredFields, visibleLogFields],
  );
  const commonFields = React.useMemo(
    () => COMMON_LOG_FIELD_ORDER
      .map((name) => filteredFields.find((field) => field.name === name))
      .filter((field): field is LogFieldDef => field !== undefined)
      .filter((field) => !visibleLogFields.includes(field.name)),
    [filteredFields, visibleLogFields],
  );
  const otherFields = React.useMemo(
    () => filteredFields.filter((field) => (
      !COMMON_LOG_FIELDS.has(field.name) && !visibleLogFields.includes(field.name)
    )),
    [filteredFields, visibleLogFields],
  );
  // SQL 检索函数（MATCH / MATCH_TEXT）由后端能力接口驱动；Fields 与 SQL 模式都注入。
  const sqlFunctions = useSqlFunctionCompletions();
  const completionItems = React.useMemo(
    () => [...sqlFunctions, ...logQueryCompletions(fields, rows, maskedFields)],
    [sqlFunctions, fields, maskedFields, rows],
  );

  React.useEffect(() => {
    const names = fields.map((field) => field.name);
    setVisibleLogFields((current) => {
      if (names.length === 0) return [];
      const next = current.filter((name) => names.includes(name));
      const populatedNames = fields
        .filter((field) => Number(field.count) > 0)
        .map((field) => field.name);
      return next.length > 0
        ? next
        : defaultLogTableFields(populatedNames.length > 0 ? populatedNames : names);
    });
  }, [fields]);

  const toggleLogFieldVisibility = React.useCallback((field: string) => {
    setVisibleLogFields((current) => (
      current.includes(field) ? current.filter((name) => name !== field) : [...current, field]
    ));
  }, []);

  const reorderLogField = React.useCallback((
    source: string,
    target: string,
    position: LogColumnDropPosition,
  ) => {
    setVisibleLogFields((current) => reorderVisibleLogFields(current, source, target, position));
  }, []);

  const insertLogFieldFilter = React.useCallback((field: string) => {
    if (!stream) return;
    const definition = fields.find((candidate) => candidate.name === field);
    if (!definition) return;
    setQueryEditorCollapsed(false);
    if (queryMode === 'sql') {
      setQuery((current) => appendSqlFieldFilter(current, stream, definition));
      return;
    }
    setFieldQuery((current) => appendLogFieldClause(current, definition));
  }, [fields, queryMode, stream]);

  const insertLogFieldValueFilter = React.useCallback((
    field: string,
    value: unknown,
    filterMode: 'include' | 'exclude',
  ) => {
    if (!stream) return;
    const definition = fields.find((candidate) => candidate.name === field);
    if (queryMode === 'fields' && definition?.dataType === 'json') return;
    setQueryEditorCollapsed(false);
    if (queryMode === 'sql') {
      setQuery((current) => appendSqlFieldValueFilter(current, stream, field, value, filterMode));
      return;
    }
    setFieldQuery((current) => appendLogFieldValueClause(current, field, value, filterMode));
  }, [fields, queryMode, stream]);

  const sqlTemplateStream = stream.trim() || 'app_logs';
  const queryTemplates = React.useMemo<LogQueryTemplate[]>(() => {
    if (queryMode === 'sql') {
      const table = escapeSqlIdentifier(sqlTemplateStream);
      return [
        {
          id: 'sql-errors',
          labelKey: 'explore.toolbar.fx_templates.errors',
          descriptionKey: 'explore.toolbar.fx_templates.errors_desc',
          statement: `SELECT * FROM "${table}"\nWHERE "level" = 'error'\nORDER BY _timestamp DESC\nLIMIT ${DEFAULT_LOG_LIMIT}`,
        },
        {
          id: 'sql-message-contains',
          labelKey: 'explore.toolbar.fx_templates.message_contains',
          descriptionKey: 'explore.toolbar.fx_templates.message_contains_desc',
          statement: `SELECT * FROM "${table}"\nWHERE "message" LIKE '%timeout%'\nORDER BY _timestamp DESC\nLIMIT ${DEFAULT_LOG_LIMIT}`,
        },
        {
          id: 'sql-count-by-level',
          labelKey: 'explore.toolbar.fx_templates.count_by_level',
          descriptionKey: 'explore.toolbar.fx_templates.count_by_level_desc',
          statement: `SELECT "level", COUNT(*) AS count\nFROM "${table}"\nGROUP BY "level"\nORDER BY count DESC\nLIMIT 20`,
        },
      ];
    }
    return [
      {
        id: 'field-errors',
        labelKey: 'explore.toolbar.fx_templates.errors',
        descriptionKey: 'explore.toolbar.fx_templates.errors_desc',
        statement: "level='error'",
      },
      {
        id: 'field-message-contains',
        labelKey: 'explore.toolbar.fx_templates.message_contains',
        descriptionKey: 'explore.toolbar.fx_templates.message_contains_desc',
        statement: "message contains 'timeout'",
      },
      {
        id: 'field-source',
        labelKey: 'explore.toolbar.fx_templates.source_filter',
        descriptionKey: 'explore.toolbar.fx_templates.source_filter_desc',
        statement: "source='otlp-http'",
      },
    ];
  }, [queryMode, sqlTemplateStream]);

  const activeQueryText = queryMode === 'sql' ? query.trim() : fieldQuery.trim();

  const insertQueryTemplate = React.useCallback((statement: string) => {
    if (queryMode === 'sql') {
      setQuery(statement);
    } else {
      setFieldQuery((current) => appendLogQueryExpression(current, statement));
    }
    setFunctionPanelOpen(false);
    setQueryEditorCollapsed(false);
  }, [queryMode]);

  const resetActiveQuery = React.useCallback(() => {
    if (queryMode === 'sql') {
      setQuery(stream.trim() ? defaultLogQuery(stream.trim()) : '');
    } else {
      setFieldQuery('');
    }
    setSelectedRow(null);
    setResultPage(1);
    setQueryResult(undefined);
    setLogCursorPage(null);
    setQueryError(null);
    setQueryEditorCollapsed(false);
  }, [queryMode, stream]);

  const clearActiveQuery = React.useCallback(() => {
    if (queryMode === 'sql') {
      setQuery('');
    } else {
      setFieldQuery('');
    }
  }, [queryMode]);

  const copyActiveQuery = React.useCallback(async () => {
    if (!activeQueryText) return;
    await copyTextToClipboard(activeQueryText);
    toast.success(t('explore.toolbar.copied_query'));
  }, [activeQueryText, t]);

  const downloadCurrentResults = React.useCallback(() => {
    if (!queryResult) return;
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    downloadJsonFile(`molesignal-logs-${timestamp}.json`, {
      queryMode,
      stream,
      query: activeQueryText,
      result: queryResult,
    });
  }, [activeQueryText, queryMode, queryResult, stream]);

  const downloadVisibleCsv = React.useCallback(() => {
    if (rows.length === 0) return;
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const columns = ['_timestamp', ...visibleLogFields.filter((field) => field !== '_timestamp')];
    downloadTextFile(
      `molesignal-logs-${timestamp}.csv`,
      recordsToCsv(rawRecords, columns),
      'text/csv;charset=utf-8',
    );
  }, [rawRecords, rows.length, visibleLogFields]);

  const downloadVisibleLog = React.useCallback(() => {
    if (rows.length === 0) return;
    const timestamp = new Date().toISOString().replace(/[:.]/g, '-');
    const columns = ['_timestamp', ...visibleLogFields.filter((field) => field !== '_timestamp')];
    downloadTextFile(
      `molesignal-logs-${timestamp}.log`,
      recordsToLogText(rawRecords, columns),
      'text/plain;charset=utf-8',
    );
  }, [rawRecords, rows.length, visibleLogFields]);

  const hasRunnableQuery = queryMode === 'fields' || query.trim().length > 0;
  const canRun = Boolean(orgId && stream.trim() && hasRunnableQuery && !queryPending);
  const activeQueryKey = logExecutionKey(queryMode, stream, query, fieldQuery);
  const queryDirty = Boolean(activeQueryKey && activeQueryKey !== lastExecutedQueryKey);

  React.useEffect(() => {
    if (previousTimeWindowRef.current === timeWindow) return;
    previousTimeWindowRef.current = timeWindow;
    if (!orgId || !stream.trim() || !hasRunnableQuery || queryPending) return;
    void executeQuery();
  }, [executeQuery, hasRunnableQuery, orgId, queryPending, stream, timeWindow]);

  const fieldState: 'loading' | 'empty' | 'error' | null =
    queryError ? 'error' : queryPending && !queryResult ? 'loading' : fields.length === 0 ? 'empty' : null;
  const fieldEmptyLabel = stream
    ? t('explore.left_panel.no_fields_from_query')
    : t('explore.left_panel.pick_stream_prompt');
  const selectedLog = selectedRow === null ? null : rows[selectedRow] ?? null;

  return (
    <div
      data-workspace="logs"
      className="flex h-[calc(100vh-var(--topbar-h)-var(--contextbar-h,0px))] min-h-0 flex-col overflow-hidden bg-bg-0"
    >
      <PageHeader
        title={t('explore.title')}
        subtitle={t('explore.subtitle')}
        className="shrink-0"
      />

      <QueryWorkbench
        className="shrink-0"
        toolbar={
          <>
            <QueryToolbarGroup>
              <QueryToolbarButton
                active={mode === MODE_OPTIONS[0].id}
                tone="blue"
                onClick={() => setMode(MODE_OPTIONS[0].id)}
                className="w-9 px-0"
                aria-label={t(MODE_OPTIONS[0].ariaKey)}
              >
                <Search aria-hidden="true" className="h-4 w-4" />
              </QueryToolbarButton>
              <HistogramToggle
                visible={showHistogram}
                label={t('explore.toolbar.histogram')}
                onVisibleChange={setShowHistogram}
              />
              <QueryToolbarButton
                active={mode === MODE_OPTIONS[1].id}
                tone="blue"
                onClick={() => setMode(MODE_OPTIONS[1].id)}
                className="w-9 px-0"
                aria-label={t(MODE_OPTIONS[1].ariaKey)}
              >
                <SlidersHorizontal aria-hidden="true" className="h-4 w-4" />
              </QueryToolbarButton>
            </QueryToolbarGroup>
            <QueryToolbarGroup aria-label={t('explore.toolbar.query_mode_aria')}>
              {LOG_QUERY_MODES.map((item) => (
                <QueryToolbarButton
                  key={item.id}
                  active={queryMode === item.id}
                  tone="orange"
                  onClick={() => {
                    setQueryMode(item.id);
                    setQueryEditorCollapsed(false);
                  }}
                >
                  {t(item.labelKey)}
                </QueryToolbarButton>
              ))}
            </QueryToolbarGroup>
            <QuerySyntaxHelp mode={queryMode} scope="logs" />
            {streams.length > 0 ? (
              <Select value={stream} onValueChange={setAutoStream}>
                <SelectTrigger
                  aria-label={t('explore.left_panel.select_stream_aria')}
                  className="h-9 w-[220px] rounded-md border-bd-1 bg-bg-1 px-2.5 font-sans text-xs font-strong text-tx-0"
                >
                  <SelectValue placeholder={t('explore.left_panel.select_stream_placeholder')} />
                </SelectTrigger>
                <SelectContent>
                  {streams.map((item) => (
                    <SelectItem key={item.id} value={item.name} className="font-sans text-xs">
                      <span className="truncate text-tx-0">{item.label || item.name}</span>
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : (
              <div className="flex h-9 w-[220px] items-center rounded-md border border-bd-1 bg-bg-1 px-2.5 font-sans text-xs font-strong text-tx-2">
                {streamsQuery.isLoading
                  ? t('explore.left_panel.loading_streams')
                  : t('explore.left_panel.no_streams')}
              </div>
            )}
            <TimezoneSelect value={tzOverride} onChange={setTzOverride} className="h-9" />
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <ChromeButton aria-label={t('explore.toolbar.more_actions_aria')}>
                  <MoreVertical className="h-3 w-3" />
                </ChromeButton>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start" className="w-56 border-bd-1 bg-bg-0">
                <DropdownMenuLabel>{t('explore.toolbar.actions_label')}</DropdownMenuLabel>
                <DropdownMenuSeparator />
                <DropdownMenuItem disabled={!activeQueryText} onSelect={() => void copyActiveQuery()}>
                  <Clipboard className="h-3.5 w-3.5" />
                  {t('explore.toolbar.copy_query')}
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={resetActiveQuery}>
                  <X className="h-3.5 w-3.5" />
                  {t('explore.toolbar.reset_query')}
                </DropdownMenuItem>
                <DropdownMenuItem disabled={!queryResult} onSelect={downloadCurrentResults}>
                  <Download className="h-3.5 w-3.5" />
                  {t('explore.toolbar.download_results')}
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <div className="ml-auto flex shrink-0 flex-wrap items-center justify-end gap-1.5">
              <QueryToolbarButton
                active={functionPanelOpen}
                aria-label={t('explore.toolbar.fx_aria')}
                onClick={() => {
                  setFunctionPanelOpen((open) => !open);
                  setQueryEditorCollapsed(false);
                }}
              >
                {t('explore.toolbar.fx')}
              </QueryToolbarButton>
              <TimeRangeChip />
              <ChromeButton
                variant="primary"
                onClick={() => void executeQuery()}
                disabled={!canRun}
                className={queryDirty ? 'bg-orange-dim text-orange-soft' : undefined}
              >
                <Play className="h-3 w-3" /> {queryPending ? t('explore.toolbar.running') : t('explore.toolbar.run')}
              </ChromeButton>
              <ChromeButton aria-label={t('explore.toolbar.refresh_aria')} onClick={() => void executeQuery()} disabled={!canRun}>
                <RefreshCw className="h-3 w-3" />
              </ChromeButton>
            </div>
          </>
        }
      >
        {functionPanelOpen && (
          <div className="mb-3 overflow-hidden rounded-lg border border-bd-1 bg-bg-1">
            <div className="border-b border-bd-0 px-4 py-3">
              <div className="font-sans text-sm font-bold text-tx-0">{t('explore.toolbar.fx_title')}</div>
              <div className="mt-1 font-sans text-xs text-tx-3">{t('explore.toolbar.fx_hint')}</div>
            </div>
            <div className="grid gap-2 p-3 md:grid-cols-3">
              {queryTemplates.map((template) => (
                <button
                  key={template.id}
                  type="button"
                  onClick={() => insertQueryTemplate(template.statement)}
                  className="min-w-0 rounded-md border border-bd-0 bg-bg-0 px-3 py-3 text-left hover:bg-bg-2 focus:bg-bg-2"
                >
                  <span className="block font-sans text-xs font-semibold text-tx-0">{t(template.labelKey)}</span>
                  <code className="mt-1.5 block overflow-hidden text-ellipsis whitespace-nowrap font-mono text-xs text-indigo-soft">
                    {template.statement.replace(/\s+/g, ' ')}
                  </code>
                  <span className="mt-1.5 block font-sans text-xs text-tx-3">{t(template.descriptionKey)}</span>
                </button>
              ))}
            </div>
          </div>
        )}
        <QueryEditorFrame
          queryRef="A"
          value={queryMode === 'sql' ? query : fieldQuery}
          onChange={queryMode === 'sql' ? setQuery : setFieldQuery}
          onClear={clearActiveQuery}
          clearLabel={t('explore.toolbar.clear_query')}
          onModEnter={() => {
            if (canRun) void executeQuery();
          }}
          language={queryMode === 'sql' ? 'sql' : 'field-query'}
          label={queryMode === 'sql' ? 'SQL' : 'Fields'}
          ariaLabel={queryMode === 'sql' ? 'SQL query editor' : 'Log field query editor'}
          placeholder={logQueryPlaceholder(queryMode, stream)}
          collapsed={queryEditorCollapsed}
          onCollapsedChange={(collapsed) => {
            setQueryEditorCollapsed(collapsed);
            if (collapsed) setFunctionPanelOpen(false);
          }}
          collapseLabel={t('explore.toolbar.collapse_editor')}
          expandLabel={t('explore.toolbar.expand_editor')}
          summary={activeQueryText || t('explore.toolbar.empty_query_summary')}
          completionItems={queryMode === 'fields' ? completionItems : sqlFunctions}
          minHeight={queryMode === 'sql' ? 240 : 180}
          maxHeight={queryMode === 'sql' ? 420 : 320}
          lineNumbers
          resizable
        />
      </QueryWorkbench>

      {/* Body: independent field and result scrollers fill the remaining viewport. */}
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <CollapsibleSidePanel
          title={t('explore.left_panel.title')}
          collapsed={fieldPanelCollapsed}
          onCollapsedChange={setFieldPanelCollapsed}
          variant="utility"
          widthClassName="w-[240px]"
          resizable
          defaultWidth={240}
          resizeLabel={t('explore.left_panel.resize')}
          bodyClassName="flex flex-col"
          collapseLabel={t('explore.left_panel.collapse')}
          expandLabel={t('explore.left_panel.expand')}
          footer={
            <div className="flex h-11 items-center justify-between border-t border-bd-0 px-2 font-sans text-xs font-strong text-tx-2">
              <span className="min-w-0 truncate">
                {t('explore.left_panel.fields_summary', {
                  shown: filteredFields.length.toLocaleString(),
                  total: fields.length.toLocaleString(),
                  visible: visibleLogFields.length.toLocaleString(),
                })}
              </span>
              <button
                aria-label={t('explore.left_panel.refresh_fields_aria')}
                className="grid h-8 w-8 place-items-center rounded-md hover:bg-bg-3 disabled:cursor-not-allowed disabled:opacity-50"
                disabled={!canRun}
                onClick={() => void executeQuery()}
              >
                <RefreshCw className="h-2.5 w-2.5" />
              </button>
            </div>
          }
        >
          <div className="px-2 pb-2">
            <div className="flex h-8 items-center gap-2 rounded-md border border-bd-1 bg-bg-1 px-2.5 font-sans text-xs">
              <Search className="h-3.5 w-3.5 text-tx-3" />
              <input
                value={fieldFilter}
                onChange={(e) => setFieldFilter(e.target.value)}
                placeholder={t('explore.left_panel.field_search_placeholder')}
                aria-label={t('explore.left_panel.filter_fields_aria')}
                className="flex-1 bg-transparent text-tx-0 placeholder:text-tx-3 focus:outline-none"
              />
            </div>
          </div>
          <div className="flex-1 overflow-auto px-1">
            {fieldState ? (
              <QueryState
                state={fieldState}
                error={queryError}
                loadingLabel={t('explore.left_panel.loading_fields')}
                emptyLabel={fieldEmptyLabel}
                className="h-full min-h-32 px-4 text-center"
              />
            ) : filteredFields.length === 0 ? (
              <QueryState
                state="empty"
                emptyLabel={t('explore.left_panel.no_matching_fields')}
                className="h-full min-h-32 px-4 text-center"
              />
            ) : (
              <TooltipProvider delayDuration={250}>
                {displayedFields.length > 0 && (
                  <SidePanelSection
                    title={t('explore.left_panel.visible_fields')}
                    count={displayedFields.length}
                  >
                    {displayedFields.map((field) => (
                      <LogFieldRow
                        key={field.name}
                        field={field}
                        filterable={isLogFieldFilterable(field, queryMode)}
                        visible
                        expanded={expandedField === field.name}
                        topValues={topLogFieldValues(rawRecords, field.name)}
                        onExpandedChange={(open) => setExpandedField(open ? field.name : null)}
                        onToggleVisibility={toggleLogFieldVisibility}
                        onInsertFilter={insertLogFieldFilter}
                        onInsertValueFilter={insertLogFieldValueFilter}
                      />
                    ))}
                  </SidePanelSection>
                )}
                {commonFields.length > 0 && (
                  <SidePanelSection
                    title={t('explore.left_panel.common_fields')}
                    count={commonFields.length}
                    className={displayedFields.length > 0 ? 'border-t border-bd-0' : undefined}
                  >
                    {commonFields.map((field) => (
                      <LogFieldRow
                        key={field.name}
                        field={field}
                        filterable={isLogFieldFilterable(field, queryMode)}
                        visible={false}
                        expanded={expandedField === field.name}
                        topValues={topLogFieldValues(rawRecords, field.name)}
                        onExpandedChange={(open) => setExpandedField(open ? field.name : null)}
                        onToggleVisibility={toggleLogFieldVisibility}
                        onInsertFilter={insertLogFieldFilter}
                        onInsertValueFilter={insertLogFieldValueFilter}
                      />
                    ))}
                  </SidePanelSection>
                )}
                {otherFields.length > 0 && (
                  <SidePanelSection
                    title={t('explore.left_panel.other_fields')}
                    count={otherFields.length}
                    className={displayedFields.length > 0 || commonFields.length > 0 ? 'border-t border-bd-0' : undefined}
                  >
                    {otherFields.map((field) => (
                      <LogFieldRow
                        key={field.name}
                        field={field}
                        filterable={isLogFieldFilterable(field, queryMode)}
                        visible={false}
                        expanded={expandedField === field.name}
                        topValues={topLogFieldValues(rawRecords, field.name)}
                        onExpandedChange={(open) => setExpandedField(open ? field.name : null)}
                        onToggleVisibility={toggleLogFieldVisibility}
                        onInsertFilter={insertLogFieldFilter}
                        onInsertValueFilter={insertLogFieldValueFilter}
                      />
                    ))}
                  </SidePanelSection>
                )}
              </TooltipProvider>
            )}
          </div>
        </CollapsibleSidePanel>

        <div className="flex min-w-0 flex-1 overflow-hidden">
          <div
            data-workspace-pane="log-results"
            className="flex min-w-0 flex-1 flex-col overflow-hidden bg-bg-0"
          >
            <div
              data-log-result-summary
              className="flex min-h-11 flex-wrap items-center gap-2 border-b border-bd-0 px-4 py-1.5 font-sans text-xs"
            >
              <span className="h-1.5 w-1.5 rounded-full bg-orange" />
              <span className="font-semibold text-tx-0">
                {t('explore.table.events_summary', {
                  events: rows.length.toLocaleString(),
                  ms: queryResult?.took_ms ?? 0,
                })}
              </span>
              <span className="text-tx-3">·</span>
              <span className="whitespace-nowrap text-tx-2">
                {formatTimeWindowLabel(timeWindow, tCommon)}
              </span>
              <RefreshCw className={`h-3 w-3 text-tx-3 ${queryPending ? 'animate-spin' : ''}`} />
            </div>
            {showHistogram && (
              <div className="border-b border-bd-0 bg-bg-1">
                <TimeSeriesChart
                  className="px-4 py-1"
                  series={[
                    {
                      id: 'log-normal',
                      name: t('explore.histogram.normal'),
                      color: 'var(--blue-soft)',
                      data: histo.map((bucket) => bucket.ok),
                      timestamps: histo.map((bucket) =>
                        Math.round((bucket.startMicros + bucket.endMicros) / 2),
                      ),
                    },
                    {
                      id: 'log-errors',
                      name: t('explore.histogram.errors'),
                      color: 'var(--orange-soft)',
                      data: histo.map((bucket) => bucket.err),
                      timestamps: histo.map((bucket) =>
                        Math.round((bucket.startMicros + bucket.endMicros) / 2),
                      ),
                    },
                  ]}
                  {...(executedTimeRange
                    ? {
                        xDomain: [
                          executedTimeRange.start,
                          executedTimeRange.end,
                        ] as [number, number],
                      }
                    : {})}
                  height={rows.length < 5 ? 72 : 96}
                  ariaLabel={t('explore.toolbar.histogram')}
                  options={{
                    drawStyle: 'bar',
                    stackMode: 'normal',
                    showPoints: 'never',
                    legendMode: 'hidden',
                    showXAxis: false,
                    showYAxis: false,
                    leftAxis: { min: 0, showGrid: false },
                  }}
                  showLegend={false}
                  onRangeSelect={selectHistogramRange}
                />
              </div>
            )}

            <QueryRecommendations
              result={queryResult}
              statement={executedStatement}
              language="sql"
              timeRangeSecs={executedRangeSecs}
              variant="inline"
            />

            <div className="flex min-h-10 items-center gap-2 border-b border-bd-0 bg-bg-0 px-3">
              <QueryToolbarGroup
                data-log-result-mode
                aria-label={t('explore.results.content')}
              >
                <span
                  aria-current="true"
                  className="inline-flex h-8 shrink-0 items-center justify-center gap-1.5 whitespace-nowrap rounded bg-blue px-3 font-sans text-xs font-strong text-white"
                >
                  <List className="h-3.5 w-3.5" />
                  {t('explore.results.content')}
                </span>
              </QueryToolbarGroup>
              <div data-log-result-actions className="ml-auto flex items-center gap-1">
                <LogColumnMenu
                  fields={fields.map((field) => field.name)}
                  visibleFields={visibleLogFields}
                  onToggleField={toggleLogFieldVisibility}
                  onReorderField={reorderLogField}
                />
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <ChromeButton aria-label={t('explore.results.density')}>
                      <Rows3 className="h-3.5 w-3.5" />
                      {t(`explore.results.density_values.${resultDensity}`)}
                    </ChromeButton>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="w-40">
                    <DropdownMenuLabel>{t('explore.results.density')}</DropdownMenuLabel>
                    <DropdownMenuSeparator />
                    <DropdownMenuRadioGroup
                      value={resultDensity}
                      onValueChange={(value) => setResultDensity(value as LogResultDensity)}
                    >
                      {(['compact', 'normal', 'comfortable'] as const).map((density) => (
                        <DropdownMenuRadioItem key={density} value={density}>
                          {t(`explore.results.density_values.${density}`)}
                        </DropdownMenuRadioItem>
                      ))}
                    </DropdownMenuRadioGroup>
                  </DropdownMenuContent>
                </DropdownMenu>
                <ChromeButton
                  onClick={() => setFieldPanelCollapsed((collapsed) => !collapsed)}
                  aria-label={fieldPanelCollapsed
                    ? t('explore.left_panel.expand')
                    : t('explore.left_panel.collapse')}
                >
                  <SlidersHorizontal className="h-3.5 w-3.5" />
                  {t('explore.left_panel.title')}
                </ChromeButton>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <ChromeButton disabled={rows.length === 0} aria-label={t('explore.results.download')}>
                      <Download className="h-3.5 w-3.5" />
                    </ChromeButton>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="w-52">
                    <DropdownMenuLabel>{t('explore.results.download')}</DropdownMenuLabel>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem onSelect={downloadVisibleCsv}>
                      <Table2 className="h-3.5 w-3.5" />
                      {t('explore.results.download_csv')}
                    </DropdownMenuItem>
                    <DropdownMenuItem onSelect={downloadVisibleLog}>
                      <FileText className="h-3.5 w-3.5" />
                      {t('explore.results.download_log')}
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            </div>
            <div className="min-h-0 flex-1 overflow-hidden">
              {queryError ? (
                <QueryState state="error" error={queryError} className="h-full min-h-0" />
              ) : queryPending && !queryResult ? (
                <QueryState
                  state="loading"
                  loadingLabel={t('explore.table.running_query')}
                  className="h-full min-h-0"
                />
              ) : rows.length === 0 ? (
                <QueryState
                  state="empty"
                  emptyLabel={t('explore.table.no_events')}
                  className="h-full min-h-0"
                />
              ) : (
                <LogListResults
                  rows={pagedRows}
                  fields={visibleLogFields}
                  timezone={fmt.tz}
                  startIndex={resultPageStart}
                  selectedIndex={selectedRow}
                  density={resultDensity}
                  onSelect={setSelectedRow}
                />
              )}
            </div>
            {cursorResults ? (
              <CursorPagination
                pageSize={logPageSize}
                pageSizeOptions={LOG_CURSOR_PAGE_SIZE_OPTIONS}
                hasPrevious={Boolean(logCursorPage?.previous_cursor)}
                hasNext={Boolean(logCursorPage?.next_cursor)}
                pending={queryPending}
                ariaLabel={t('explore.table.pagination_aria')}
                pageSizeAriaLabel={t('explore.table.events_per_page_aria')}
                previousLabel={t('explore.table.prev_page')}
                nextLabel={t('explore.table.next_page')}
                onPrevious={() => changeLogPage('previous')}
                onNext={() => changeLogPage('next')}
                onPageSizeChange={changeLogPageSize}
              />
            ) : (
              <ResultPagination
                page={activeResultPage}
                pageCount={resultPageCount}
                pageSize={resultPageSize}
                pageSizeOptions={LOG_PAGE_SIZE_OPTIONS}
                pageLabel={t('explore.table.page_summary', {
                  page: activeResultPage,
                  pages: resultPageCount,
                })}
                ariaLabel={t('explore.table.pagination_aria')}
                pageSizeAriaLabel={t('explore.table.events_per_page_aria')}
                firstAriaLabel={t('explore.table.first_page_aria')}
                previousAriaLabel={t('explore.table.prev_page_aria')}
                nextAriaLabel={t('explore.table.next_page_aria')}
                lastAriaLabel={t('explore.table.last_page_aria')}
                onPageChange={changeResultPage}
                onPageSizeChange={changeResultPageSize}
              />
            )}
          </div>

          {selectedLog && selectedRow !== null && (
            <>
              <button
                type="button"
                aria-label={t('explore.detail.close')}
                tabIndex={-1}
                onClick={() => setSelectedRow(null)}
                className="fixed bottom-0 left-0 right-0 top-topbar z-[55] cursor-default border-0 bg-transparent p-0 focus:outline-none"
              />
              <aside
                aria-label={t('explore.detail.drawer_aria')}
                className="fixed bottom-0 right-0 top-topbar z-[60] min-h-0 w-[34vw] min-w-[420px] max-w-[660px] border-l border-bd-1 bg-bg-0 shadow-drawer data-[state=open]:animate-slide-in-right"
                data-state="open"
              >
                <LogDetail
                  log={selectedLog}
                  index={selectedRow}
                  total={rows.length}
                  stream={stream}
                  contextRows={rows.slice(Math.max(0, selectedRow - 5), Math.min(rows.length, selectedRow + 6))}
                  visibleFields={visibleLogFields}
                  onInsertValueFilter={insertLogFieldValueFilter}
                  onToggleVisibility={toggleLogFieldVisibility}
                  onSelectContext={(log) => {
                    const nextIndex = rows.indexOf(log);
                    if (nextIndex >= 0) setSelectedRow(nextIndex);
                  }}
                  onClose={() => setSelectedRow(null)}
                  onPrev={() => setSelectedRow(Math.max(0, selectedRow - 1))}
                  onNext={() => setSelectedRow(Math.min(rows.length - 1, selectedRow + 1))}
                />
              </aside>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

function LogFieldRow({
  field,
  filterable,
  visible,
  expanded,
  topValues,
  onExpandedChange,
  onToggleVisibility,
  onInsertFilter,
  onInsertValueFilter,
}: {
  field: LogFieldDef;
  filterable: boolean;
  visible: boolean;
  expanded: boolean;
  topValues: LogTopValue[];
  onExpandedChange: (open: boolean) => void;
  onToggleVisibility: (field: string) => void;
  onInsertFilter: (field: string) => void;
  onInsertValueFilter: (field: string, value: unknown, mode: 'include' | 'exclude') => void;
}) {
  const { t } = useTranslation('logs');
  const toggleLabel = visible
    ? t('explore.left_panel.hide_field_aria', { name: field.name })
    : t('explore.left_panel.show_field_aria', { name: field.name });
  const addLabel = filterable
    ? t('explore.left_panel.add_field_aria', { name: field.name })
    : t('explore.left_panel.json_query_required', { name: field.name });

  return (
    <div className={`border-b border-bd-0/70 ${expanded ? 'bg-bg-2' : ''}`}>
      <div className="group flex min-h-9 items-center gap-1 px-1.5 font-sans text-xs font-strong hover:bg-bg-3">
        <button
          type="button"
          onClick={() => onExpandedChange(!expanded)}
          className="flex min-w-0 flex-1 items-center gap-2 rounded px-1 py-1.5 text-left focus:bg-bg-3"
        >
          <ChevronRight className={`h-3 w-3 shrink-0 text-tx-3 transition-transform ${expanded ? 'rotate-90' : ''}`} />
          <span className={`w-4 shrink-0 text-center text-xs font-bold ${TYPE_COLOR[field.type]}`}>
            {TYPE_GLYPH[field.type]}
          </span>
          <span className="min-w-0 flex-1 truncate text-tx-0">{field.name}</span>
          <span className="type-micro shrink-0 font-mono font-normal text-tx-3">{field.count}</span>
        </button>
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              onClick={() => onToggleVisibility(field.name)}
              className="grid h-7 w-7 shrink-0 place-items-center rounded text-tx-3 opacity-0 hover:bg-bg-4 hover:text-tx-0 focus:opacity-100 group-hover:opacity-100"
              aria-label={toggleLabel}
            >
              {visible ? <Eye className="h-3 w-3" /> : <EyeOff className="h-3 w-3" />}
            </button>
          </TooltipTrigger>
          <TooltipContent>{toggleLabel}</TooltipContent>
        </Tooltip>
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              onClick={() => onInsertFilter(field.name)}
              disabled={!filterable}
              className="grid h-7 w-7 shrink-0 place-items-center rounded text-tx-3 opacity-0 hover:bg-bg-4 hover:text-blue-soft focus:opacity-100 disabled:cursor-not-allowed disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-tx-3 group-hover:opacity-100"
              aria-label={addLabel}
            >
              <Plus className="h-3 w-3" />
            </button>
          </TooltipTrigger>
          <TooltipContent>{addLabel}</TooltipContent>
        </Tooltip>
      </div>
      {expanded && (
        <div className="border-t border-bd-0 bg-bg-1 px-2 pb-2 pt-1.5">
          <div className="type-micro flex items-center justify-between px-1 py-1 font-sans font-semibold uppercase tracking-wide text-tx-3">
            <span>{t('explore.left_panel.top_values')}</span>
            <span>{t('explore.left_panel.value_count')}</span>
          </div>
          {topValues.length === 0 ? (
            <div className="px-1 py-2 font-sans text-xs text-tx-3">
              {t('explore.left_panel.no_top_values')}
            </div>
          ) : (
            <div className="space-y-0.5">
              {topValues.map((topValue) => (
                <div
                  key={`${typeof topValue.value}:${topValue.label}`}
                  className="group/value flex min-h-8 items-center gap-1 rounded px-1 hover:bg-bg-3"
                >
                  <span className="type-micro min-w-0 flex-1 truncate font-mono text-tx-1" title={topValue.label}>
                    {topValue.label}
                  </span>
                  <span className="type-micro w-7 shrink-0 text-right font-mono text-tx-3">{topValue.count}</span>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        onClick={() => onInsertValueFilter(field.name, topValue.value, 'include')}
                        disabled={!filterable}
                        className="grid h-6 w-6 place-items-center rounded text-tx-3 opacity-0 hover:bg-bg-4 hover:text-blue-soft focus:opacity-100 disabled:cursor-not-allowed disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-tx-3 group-hover/value:opacity-100"
                        aria-label={t('explore.left_panel.include_value_aria', { value: topValue.label })}
                      >
                        <Filter className="h-3 w-3" />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent>{t('explore.left_panel.include_value')}</TooltipContent>
                  </Tooltip>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        onClick={() => onInsertValueFilter(field.name, topValue.value, 'exclude')}
                        disabled={!filterable}
                        className="grid h-6 w-6 place-items-center rounded text-tx-3 opacity-0 hover:bg-bg-4 hover:text-orange-soft focus:opacity-100 disabled:cursor-not-allowed disabled:opacity-30 disabled:hover:bg-transparent disabled:hover:text-tx-3 group-hover/value:opacity-100"
                        aria-label={t('explore.left_panel.exclude_value_aria', { value: topValue.label })}
                      >
                        <Minus className="h-3 w-3" />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent>{t('explore.left_panel.exclude_value')}</TooltipContent>
                  </Tooltip>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function LogDetail({
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
}: {
  log: LogEntry;
  index: number;
  total: number;
  stream: string;
  contextRows: LogEntry[];
  visibleFields: string[];
  onInsertValueFilter: (field: string, value: unknown, mode: 'include' | 'exclude') => void;
  onToggleVisibility: (field: string) => void;
  onSelectContext: (log: LogEntry) => void;
  onClose: () => void;
  onPrev: () => void;
  onNext: () => void;
}) {
  const { t } = useTranslation('logs');
  const navigate = useNavigate();
  const [tab, setTab] = React.useState<'overview' | 'fields' | 'json' | 'context'>('overview');
  const [copied, setCopied] = React.useState(false);
  const copyTimerRef = React.useRef<number | null>(null);
  const fullJson = log.raw;

  React.useEffect(() => () => {
    if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
  }, []);

  const handleCopy = React.useCallback(async () => {
    await copyTextToClipboard(logRecordJson(fullJson));
    setCopied(true);
    if (copyTimerRef.current !== null) window.clearTimeout(copyTimerRef.current);
    copyTimerRef.current = window.setTimeout(() => setCopied(false), 1400);
  }, [fullJson]);

  const handleDownload = React.useCallback(() => {
    const safeTimestamp = log.ts.replace(/[^0-9A-Za-z_-]+/g, '-').replace(/^-+|-+$/g, '') || String(index + 1);
    downloadJsonFile(`molesignal-log-${safeTimestamp}.json`, fullJson);
  }, [fullJson, index, log.ts]);

  const handleJump = React.useCallback((to: string) => navigate(to), [navigate]);
  const message = primaryLogMessage(log.raw);
  const source = logSourceLabel(log.raw);
  const level = logLevelLabel(log.raw, log.level);
  const primaryFields = visibleFields
    .filter((field) => isPresent(log.raw[field]))
    .slice(0, 8);
  const detailTabs = [
    { key: 'overview', label: t('explore.detail.tabs.overview'), icon: List },
    { key: 'fields', label: t('explore.detail.tabs.fields'), icon: Columns3 },
    { key: 'json', label: t('explore.detail.tabs.json'), icon: Braces },
    { key: 'context', label: t('explore.detail.tabs.context'), icon: Rows3 },
  ] as const;

  return (
    <div className="flex h-full min-h-0 flex-col overflow-hidden bg-bg-0">
      <div className="flex items-center gap-2.5 border-b border-bd-0 bg-bg-1 px-4 py-2.5">
        <span className="type-data font-sans font-bold text-tx-0">{t('explore.detail.title')}</span>
        <Pill>
          #{index + 1} / {total}
        </Pill>
        <ChromeButton className="ml-auto" onClick={onClose} aria-label={t('explore.detail.close')} title={t('explore.detail.close')}>
          <X className="h-3 w-3" />
        </ChromeButton>
      </div>

      <div className="flex items-center border-b border-bd-0 bg-bg-1 px-4">
        <div className="flex gap-0.5 py-2">
          {detailTabs.map(({ key: tabKey, label, icon: Icon }) => (
            <button
              key={tabKey}
              onClick={() => setTab(tabKey)}
              className={`flex items-center gap-1.5 rounded px-2.5 py-1.5 font-sans text-xs font-strong ${
                tab === tabKey ? 'bg-bg-3 text-tx-0' : 'text-tx-2 hover:text-tx-0'
              }`}
            >
              <Icon className="h-3 w-3" />
              {label}
            </button>
          ))}
        </div>
        <div className="ml-auto flex gap-1.5">
          <CopyIconButton
            label={t('explore.detail.copy_json')}
            copied={copied}
            copiedLabel={t('explore.detail.copied_json')}
            onClick={() => void handleCopy()}
          />
          <ChromeButton aria-label={t('explore.detail.download_json')} title={t('explore.detail.download_json')} onClick={handleDownload}>
            <Download className="h-3 w-3" />
          </ChromeButton>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-4">
        {tab === 'overview' && (
          <div className="space-y-4 font-sans text-xs">
            <section className="rounded-md border border-bd-0 bg-bg-1 p-3">
              <div className="flex items-center gap-2">
                <span className={`type-micro rounded px-1.5 py-0.5 font-mono font-semibold ${levelToneClass(level)}`}>
                  {level}
                </span>
                <span className="type-micro font-mono text-tx-2">{log.ts}</span>
              </div>
              <div className="mt-3 text-sm font-semibold leading-6 text-tx-0">
                {message.value || compactRecord(log.raw)}
              </div>
              <div className="mt-2 flex items-center gap-2 text-tx-3">
                <span>{source}</span>
                {message.field && <span>· {message.field}</span>}
              </div>
            </section>
            <section>
              <div className="mb-2 font-semibold uppercase tracking-wide text-tx-3">
                {t('explore.detail.overview_metadata')}
              </div>
              <dl className="overflow-hidden rounded-md border border-bd-0">
                <DetailMetaRow label={t('explore.detail.stream')} value={stream} />
                <DetailMetaRow label={t('explore.detail.time')} value={log.ts} />
                <DetailMetaRow label={t('explore.detail.level')} value={level} />
                <DetailMetaRow label={t('explore.detail.source')} value={source} />
              </dl>
            </section>
            {primaryFields.length > 0 && (
              <section>
                <div className="mb-2 font-semibold uppercase tracking-wide text-tx-3">
                  {t('explore.detail.primary_fields')}
                </div>
                <div className="overflow-hidden rounded-md border border-bd-0">
                  {primaryFields.map((field) => (
                    <DetailMetaRow
                      key={field}
                      label={field}
                      value={formatLogFieldValue(log.raw[field])}
                    />
                  ))}
                </div>
              </section>
            )}
          </div>
        )}
        {tab === 'fields' && (
          <KvTable
            obj={fullJson}
            onJump={handleJump}
            visibleFields={visibleFields}
            onInclude={(field, value) => onInsertValueFilter(field, value, 'include')}
            onExclude={(field, value) => onInsertValueFilter(field, value, 'exclude')}
            onToggleVisibility={onToggleVisibility}
          />
        )}
        {tab === 'json' && (
          <JsonTree obj={fullJson} record={fullJson} onJump={handleJump} />
        )}
        {tab === 'context' && (
          <div className="space-y-2">
            <div className="font-sans text-xs leading-5 text-tx-3">
              {t('explore.detail.context_description')}
            </div>
            <div className="overflow-hidden rounded-md border border-bd-0">
              {contextRows.map((contextLog, contextIndex) => {
                const contextMessage = primaryLogMessage(contextLog.raw);
                const active = contextLog === log;
                return (
                  <button
                    key={`${contextLog.ts}-${contextIndex}`}
                    type="button"
                    onClick={() => onSelectContext(contextLog)}
                    className={`grid w-full grid-cols-[130px_minmax(0,1fr)] gap-3 border-b border-bd-0 px-3 py-2 text-left font-sans text-xs last:border-b-0 hover:bg-bg-2 ${
                      active ? 'bg-indigo-dim text-indigo-soft' : ''
                    }`}
                  >
                    <span className="type-micro font-mono text-tx-3">{contextLog.ts}</span>
                    <span className="min-w-0">
                      <span className="block truncate text-tx-0">{contextMessage.value || compactRecord(contextLog.raw)}</span>
                      <span className="type-micro mt-0.5 block truncate text-tx-3">{logSourceLabel(contextLog.raw)}</span>
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
        )}
      </div>

      <div className="flex items-center justify-between border-t border-bd-0 bg-bg-1 px-4 py-2">
        <ChromeButton onClick={onPrev} disabled={index === 0}>
          <ArrowLeft className="h-3 w-3" /> {t('explore.detail.prev')}
        </ChromeButton>
        <span className="font-sans text-xs text-tx-3">
          {t('explore.detail.position', { current: index + 1, total })}
        </span>
        <ChromeButton onClick={onNext} disabled={index === total - 1}>
          {t('explore.detail.next')} <ArrowRight className="h-3 w-3" />
        </ChromeButton>
      </div>
    </div>
  );
}

function DetailMetaRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="grid grid-cols-[140px_minmax(0,1fr)] border-b border-bd-0 last:border-b-0">
      <dt className="bg-bg-2 px-3 py-2 text-tx-2">{label}</dt>
      <dd className="min-w-0 whitespace-pre-wrap break-words px-3 py-2 text-tx-0">{value || '—'}</dd>
    </div>
  );
}

type LogFieldJumpHandler = (to: string) => void;

function JsonTree({
  obj,
  record = obj,
  depth = 0,
  path = '',
  onJump,
}: {
  obj: Record<string, unknown>;
  record?: Record<string, unknown>;
  depth?: number;
  path?: string;
  onJump?: LogFieldJumpHandler | undefined;
}) {
  return (
    <div className="font-sans text-xs font-strong leading-[1.7]">
      <div>{'{'}</div>
      {Object.entries(obj).map(([k, v], i, arr) => {
        const childPath = path ? `${path}.${k}` : k;
        return (
          <JsonRow
            key={childPath}
            k={k}
            path={childPath}
            v={v}
            last={i === arr.length - 1}
            depth={depth + 1}
            record={record}
            onJump={onJump}
          />
        );
      })}
      <div>{'}'}</div>
    </div>
  );
}

function JsonRow({
  k,
  path,
  v,
  last,
  depth,
  record,
  onJump,
}: {
  k: string;
  path: string;
  v: unknown;
  last: boolean;
  depth: number;
  record: Record<string, unknown>;
  onJump?: LogFieldJumpHandler | undefined;
}) {
  const [open, setOpen] = React.useState(depth <= 1);
  const isObj = v !== null && typeof v === 'object' && !Array.isArray(v);
  const isArr = Array.isArray(v);
  const indent = depth * 16;
  const valColor =
    typeof v === 'string' ? 'text-green-soft' : typeof v === 'number' ? 'text-orange-soft' : typeof v === 'boolean' ? 'text-blue-soft' : 'text-tx-1';

  if (isObj || isArr) {
    return (
      <div style={{ paddingLeft: indent }}>
        <button type="button" onClick={() => setOpen(!open)} className="flex cursor-pointer gap-1 text-left">
          <ChevronRight className={`h-3 w-3 shrink-0 text-tx-3 transition-transform ${open ? 'rotate-90' : ''}`} />
          <span className="text-tx-2">{k}:</span>
          <span className="text-tx-3">
            {isArr ? `[ ${(v as unknown[]).length} items ]` : `{ ${Object.keys(v as object).length} keys }`}
          </span>
        </button>
        {open && (isObj ? <JsonTree obj={v as Record<string, unknown>} record={record} depth={depth} path={path} onJump={onJump} /> : null)}
      </div>
    );
  }
  return (
    <div className="flex items-start gap-1.5" style={{ paddingLeft: indent }}>
      <span className="text-tx-2">{k}:</span>
      <LogFieldValue field={path} value={v} record={record} onJump={onJump} className={valColor} />
      {!last && <span className="text-tx-3">,</span>}
    </div>
  );
}

function KvTable({
  obj,
  onJump,
  visibleFields = [],
  onInclude,
  onExclude,
  onToggleVisibility,
}: {
  obj: Record<string, unknown>;
  onJump?: LogFieldJumpHandler;
  visibleFields?: string[];
  onInclude?: (field: string, value: unknown) => void;
  onExclude?: (field: string, value: unknown) => void;
  onToggleVisibility?: (field: string) => void;
}) {
  const { t } = useTranslation('logs');
  function flatten(o: Record<string, unknown>, prefix = ''): Array<[string, unknown]> {
    const out: Array<[string, unknown]> = [];
    for (const [k, v] of Object.entries(o)) {
      const key = prefix ? `${prefix}.${k}` : k;
      if (v && typeof v === 'object' && !Array.isArray(v)) {
        out.push(...flatten(v as Record<string, unknown>, key));
      } else {
        out.push([key, Array.isArray(v) ? JSON.stringify(v) : v]);
      }
    }
    return out;
  }
  const rows = flatten(obj);
  return (
    <TooltipProvider delayDuration={250}>
      <table className="w-full border-collapse font-sans text-xs font-strong">
        <thead>
          <tr>
            <th className={`w-[190px] border-b border-bd-0 px-2.5 py-1.5 text-left ${uiTableHeaderClass}`}>
              {t('explore.detail.field_column')}
            </th>
            <th className={`border-b border-bd-0 px-2.5 py-1.5 text-left ${uiTableHeaderClass}`}>
              {t('explore.detail.value_column')}
            </th>
            <th className={`w-[116px] border-b border-bd-0 px-2.5 py-1.5 text-right ${uiTableHeaderClass}`}>
              {t('explore.detail.actions_column')}
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map(([k, v]) => (
            <tr key={k} className="group border-b border-bd-0 hover:bg-bg-1">
              <td className="px-2.5 py-1.5 align-top text-tx-2">{k}</td>
              <td className="min-w-0 px-2.5 py-1.5 align-top">
                <LogFieldValue
                  field={k}
                  value={v}
                  record={obj}
                  onJump={onJump}
                  className={typeof v === 'number' ? 'text-orange-soft' : typeof v === 'boolean' ? 'text-blue-soft' : 'text-green-soft'}
                />
              </td>
              <td className="px-1.5 py-1 align-top">
                <div className="flex justify-end opacity-0 focus-within:opacity-100 group-hover:opacity-100">
                  <FieldActionButton
                    label={t('explore.detail.include_filter')}
                    icon={Filter}
                    disabled={!onInclude || !isPresent(v)}
                    onClick={() => onInclude?.(k, v)}
                  />
                  <FieldActionButton
                    label={t('explore.detail.exclude_filter')}
                    icon={Minus}
                    disabled={!onExclude || !isPresent(v)}
                    onClick={() => onExclude?.(k, v)}
                  />
                  <FieldActionButton
                    label={t('explore.detail.copy_value')}
                    icon={Clipboard}
                    onClick={() => {
                      void copyTextToClipboard(formatLogFieldValue(v)).then(() => {
                        toast.success(t('explore.detail.copied_value'));
                      });
                    }}
                  />
                  <FieldActionButton
                    label={visibleFields.includes(k)
                      ? t('explore.detail.hide_column')
                      : t('explore.detail.show_column')}
                    icon={visibleFields.includes(k) ? EyeOff : Eye}
                    disabled={!onToggleVisibility}
                    onClick={() => onToggleVisibility?.(k)}
                  />
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </TooltipProvider>
  );
}

function FieldActionButton({
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
          onClick={onClick}
          disabled={disabled}
          aria-label={label}
          className="grid h-7 w-7 place-items-center rounded text-tx-3 hover:bg-bg-3 hover:text-tx-0 disabled:cursor-not-allowed disabled:opacity-30"
        >
          <Icon className="h-3 w-3" />
        </button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

function LogFieldValue({
  field,
  value,
  record,
  onJump,
  className,
}: {
  field: string;
  value: unknown;
  record: Record<string, unknown>;
  onJump?: LogFieldJumpHandler | undefined;
  className: string;
}) {
  const { t } = useTranslation('logs');
  const display = value === null ? 'null' : value === undefined ? 'undefined' : formatLogFieldValue(value);

  // Phase 6 M2: cross-signal handles (trace_id / span_id / service / host)
  // get the SignalReference HoverCard with 2-3 jump actions instead of a
  // single-destination button. Uses the shared label-name detection so
  // Metrics / Logs / Trace span attributes all recognize the same
  // aliases. Tied to brief Principle #3.
  const signalType = isPresent(value) ? detectSignalTypeForLabel(leafFieldName(field)) : null;
  if (signalType) {
    return (
      <SignalReference
        type={signalType}
        value={String(value)}
        labelName={field}
        labels={stringLabelsFromRecord(record)}
        className={className}
      >
        {display}
      </SignalReference>
    );
  }

  // Other fields: keep the legacy single-jump button if the field jump
  // config matches; otherwise plain text.
  const handleJump = onJump;
  const jump = handleJump ? resolveLogFieldJump(record, field, value) : null;
  if (!jump || !handleJump) {
    return <span className={`${className} whitespace-pre-wrap break-all`}>{display}</span>;
  }
  return (
    <button
      type="button"
      onClick={(event) => {
        event.stopPropagation();
        handleJump(jump.to);
      }}
      className={`${className} inline whitespace-pre-wrap break-all text-left underline decoration-current/40 underline-offset-2 hover:brightness-110 focus:bg-indigo-dim focus:text-indigo-soft`}
      title={t(jump.titleKey)}
    >
      {display}
    </button>
  );
}
