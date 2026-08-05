import {
  Activity,
  AlertCircle,
  Copy,
  ExternalLink,
  Filter,
  FilterX,
  Server,
  type LucideIcon,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';
import { encodeFilters } from '@/shell/UrlHydration';
import { useFiltersStore, type GlobalFilter } from '@/stores/useFiltersStore';

/**
 * SignalReference — brief Principle #3 "Continuity across signals" tech
 * landing.
 *
 * Wrap any inline mention of a `trace_id` / `service` / `host` / `span` /
 * `stream` in this component. Clicking (or right-clicking) the value opens
 * one consistent contextual-pivot menu. Jumps preserve the surrounding time
 * window, source trace, and pinned filters through readable URL parameters.
 *
 * Per Phase 3 IA URL schema: `?from=&to=&trace_id=&service=&host=&stream=`
 * — keep changes here in sync with `web/src/shell/UrlHydration.ts` when
 * that lands.
 */

export type SignalReferenceType = 'trace_id' | 'span_id' | 'service' | 'host' | 'stream';
export type SignalReferenceStreamType = 'logs' | 'metrics' | 'traces' | 'profiles' | 'extend';

/**
 * Shared label-name → SignalReferenceType detection. Used by the Metrics
 * legend, Trace span attribute lists, and any other surface that needs to
 * decide whether a `key="value"` pair deserves a HoverCard. Aligned with
 * `Logs.tsx` aliases so a user's mental model stays consistent across
 * signal views.
 */
const TRACE_ID_LABEL_ALIASES = ['trace_id', 'traceid', 'trace.id'];
const SPAN_ID_LABEL_ALIASES = ['span_id', 'spanid', 'span.id'];
const SERVICE_LABEL_ALIASES = ['service', 'service_name', 'service.name', 'svc'];
const HOST_LABEL_ALIASES = ['host', 'host_name', 'hostname', 'host.name', 'node', 'instance'];
const STREAM_LABEL_ALIASES = ['stream', 'stream_name'];

export function detectSignalTypeForLabel(label: string): SignalReferenceType | null {
  const lower = label.toLowerCase();
  if (TRACE_ID_LABEL_ALIASES.includes(lower)) return 'trace_id';
  if (SPAN_ID_LABEL_ALIASES.includes(lower)) return 'span_id';
  if (SERVICE_LABEL_ALIASES.includes(lower)) return 'service';
  if (HOST_LABEL_ALIASES.includes(lower)) return 'host';
  if (STREAM_LABEL_ALIASES.includes(lower)) return 'stream';
  return null;
}

export interface SignalReferenceTime {
  from: string;
  to: string;
}

export interface SignalReferenceSource {
  type: 'trace';
  id: string;
}

export interface SignalReferenceOptions {
  labelName?: string | undefined;
  labels?: Record<string, string> | undefined;
  metricQuery?: string | undefined;
  streamType?: SignalReferenceStreamType | undefined;
  streamId?: string | undefined;
  source?: SignalReferenceSource | undefined;
}

interface SignalReferenceProps extends SignalReferenceOptions {
  type: SignalReferenceType;
  value: string;
  /** Time window to propagate. If omitted, the destination keeps its own
   *  default; recommend passing whenever the surrounding row already has
   *  a definite time scope (e.g. a trace timeline). */
  time?: SignalReferenceTime | undefined;
  /** Override the rendered text — defaults to `value`. Useful when the
   *  display is a short prefix (`abc123` for the full `abc123...def`). */
  children?: React.ReactNode | undefined;
  className?: string | undefined;
  showIcon?: boolean | undefined;
}

const TYPE_META: Record<SignalReferenceType, { icon: LucideIcon; labelKey: string }> = {
  trace_id: { icon: Activity, labelKey: 'signal_reference.types.trace' },
  span_id: { icon: AlertCircle, labelKey: 'signal_reference.types.span' },
  service: { icon: Server, labelKey: 'signal_reference.types.service' },
  host: { icon: Server, labelKey: 'signal_reference.types.host' },
  stream: { icon: Server, labelKey: 'signal_reference.types.stream' },
};

export function SignalReference({
  type,
  value,
  labelName,
  labels,
  metricQuery,
  streamType,
  streamId,
  source,
  time,
  children,
  className,
  showIcon = true,
}: SignalReferenceProps) {
  const { t } = useTranslation('shell');
  const meta = TYPE_META[type];
  const Icon = meta.icon;
  const [open, setOpen] = React.useState(false);
  const [copiedKey, setCopiedKey] = React.useState<string | null>(null);
  const globalFilters = useFiltersStore((s) => s.filters);
  const setFilter = useFiltersStore((s) => s.setFilter);
  // Pin this signal as a cross-page filter under its label name (or its type).
  const filterKey = (labelName?.trim() || type).toLowerCase();
  const isFiltered = globalFilters.some(
    (f) => f.key === filterKey && f.value === value && f.operator !== '!=',
  );
  const isExcluded = globalFilters.some(
    (f) => f.key === filterKey && f.value === value && f.operator === '!=',
  );
  const context = React.useMemo(
    () => buildSignalContext(type, value, { labelName, labels, metricQuery }),
    [labelName, labels, metricQuery, type, value],
  );
  const jumps = React.useMemo(
    () => buildSignalJumps(
      type,
      value,
      time,
      { labelName, labels, metricQuery, streamType, streamId, source },
      globalFilters,
    ),
    [globalFilters, labelName, labels, metricQuery, source, streamId, streamType, time, type, value],
  );

  const handleCopy = React.useCallback(async (copyValue: string, key: string) => {
    try {
      await navigator.clipboard.writeText(copyValue);
      setCopiedKey(key);
      window.setTimeout(() => setCopiedKey(null), 1200);
    } catch {
      // clipboard blocked — skip
    }
  }, []);

  const copyLabelKey =
    type === 'span_id'
      ? 'signal_reference.actions.copy_span_id'
      : type === 'trace_id'
        ? 'signal_reference.actions.copy_trace_id'
        : type === 'service'
          ? 'signal_reference.actions.copy_service'
          : 'signal_reference.actions.copy_value';

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          data-signal-type={type}
          // Phase 6 M2: prevent the trigger click from bubbling to any
          // outer row-level onClick (Streams / Traces tables both use
          // onRowClick to navigate). The popover actions inside the Portal
          // are unaffected — they live outside the row's DOM.
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => event.stopPropagation()}
          onContextMenu={(event) => {
            event.preventDefault();
            event.stopPropagation();
            setOpen(true);
          }}
          className={cn(
            'inline-flex items-center gap-1 rounded font-sans tabular-nums',
            'text-indigo-soft underline decoration-dotted decoration-1 underline-offset-2',
            'transition-all duration-fast ease-default hover:text-indigo hover:decoration-solid active:scale-[0.97]',
            'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo focus-visible:ring-offset-1 focus-visible:ring-offset-bg-1',
            className,
          )}
        >
          {showIcon && <Icon aria-hidden className="h-3 w-3 stroke-[1.6]" />}
          <span>{children ?? value}</span>
        </button>
      </PopoverTrigger>
      <PopoverContent
        side="top"
        align="start"
        sideOffset={6}
        className="w-72 border-bd-1 bg-surface p-0 shadow-popup"
      >
        <header className="border-b border-bd-0 px-3 py-2.5">
          <div className="flex min-w-0 items-center gap-1.5">
            <Icon aria-hidden className="h-3.5 w-3.5 stroke-[1.6] text-tx-2" />
            <span className="font-sans text-xs font-strong uppercase tracking-wider text-tx-3">
              {t(meta.labelKey)}
            </span>
          </div>
          <div className="mt-1.5 break-all font-sans text-xs font-semibold tabular-nums text-tx-0">
            {type === 'span_id' && context.operation ? context.operation : value}
          </div>
          {type === 'span_id' && context.operation && (
            <div className="mt-0.5 break-all font-mono text-xs tabular-nums text-tx-3">{value}</div>
          )}
        </header>
        <div className="py-1">
          <button
            type="button"
            onClick={() => setFilter(filterKey, value, '=')}
            disabled={isFiltered}
            className="flex h-8 w-full items-center gap-2 px-3 text-left font-sans text-xs text-tx-1 hover:bg-bg-3 hover:text-tx-0 disabled:cursor-default disabled:text-tx-3 disabled:hover:bg-transparent focus-visible:bg-bg-3 focus-visible:outline-none"
            data-testid="signal-pin-filter"
          >
            <Filter className="h-3.5 w-3.5 text-tx-3" />
            <span>{t(isFiltered ? 'signal_reference.actions.filtered' : 'signal_reference.actions.add_filter')}</span>
          </button>
          <button
            type="button"
            onClick={() => setFilter(filterKey, value, '!=')}
            disabled={isExcluded}
            className="flex h-8 w-full items-center gap-2 px-3 text-left font-sans text-xs text-tx-1 hover:bg-bg-3 hover:text-tx-0 disabled:cursor-default disabled:text-tx-3 disabled:hover:bg-transparent focus-visible:bg-bg-3 focus-visible:outline-none"
            data-testid="signal-exclude-filter"
          >
            <FilterX className="h-3.5 w-3.5 text-tx-3" />
            <span>{t(isExcluded ? 'signal_reference.actions.excluded' : 'signal_reference.actions.exclude_value')}</span>
          </button>
          <button
            type="button"
            onClick={() => void handleCopy(value, 'value')}
            className="flex h-8 w-full items-center gap-2 px-3 text-left font-sans text-xs text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:outline-none"
            data-testid="signal-copy"
          >
            <Copy className="h-3.5 w-3.5 text-tx-3" />
            <span>{t(copiedKey === 'value' ? 'signal_reference.actions.copied' : copyLabelKey)}</span>
          </button>
          {type === 'span_id' && context.traceId && (
            <button
              type="button"
              onClick={() => void handleCopy(context.traceId!, 'trace')}
              className="flex h-8 w-full items-center gap-2 px-3 text-left font-sans text-xs text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:outline-none"
            >
              <Copy className="h-3.5 w-3.5 text-tx-3" />
              <span>{t(copiedKey === 'trace' ? 'signal_reference.actions.copied' : 'signal_reference.actions.copy_trace_id')}</span>
            </button>
          )}
        </div>
        <ul className="border-t border-bd-0 py-1">
          {jumps.map((jump) => (
            <li key={`${jump.id}:${jump.to}`}>
              <Link
                to={jump.to}
                onClick={() => setOpen(false)}
                className={cn(
                  'flex items-center gap-2 px-3 py-1.5 font-sans text-xs text-tx-1',
                  'hover:bg-bg-3 hover:text-tx-0',
                  'focus-visible:bg-bg-3 focus-visible:outline-none focus-visible:text-tx-0',
                )}
              >
                <ExternalLink className="h-3 w-3 shrink-0 text-tx-3" />
                <span className="flex-1 truncate">{t(jump.labelKey)}</span>
                <span
                  className={cn(
                    'shrink-0 rounded px-1.5 py-0.5 text-type-micro leading-none',
                    jump.relation === 'exact'
                      ? 'bg-green-dim text-green-soft'
                      : 'bg-bg-3 text-tx-3',
                  )}
                >
                  {t(`signal_reference.relations.${jump.relation}`)}
                </span>
              </Link>
            </li>
          ))}
        </ul>
      </PopoverContent>
    </Popover>
  );
}

export type SignalJumpKind = 'traces' | 'logs' | 'metrics' | 'stream';

export interface SignalJumpAction {
  id: SignalJumpKind;
  labelKey: string;
  to: string;
  relation: 'exact' | 'context';
}

export function buildSignalJumps(
  type: SignalReferenceType,
  value: string,
  time: SignalReferenceTime | undefined,
  options: SignalReferenceOptions = {},
  filters: GlobalFilter[] = [],
): SignalJumpAction[] {
  const params = new URLSearchParams();
  if (time) {
    params.set('from', time.from);
    params.set('to', time.to);
    params.set('time', `${time.from}..${time.to}`);
  }
  if (options.source) {
    params.set('source', options.source.type);
    params.set('source_id', options.source.id);
  }
  // Pinned investigation filters ride along: carried in the URL so the
  // destination loader can restore them, and merged into the generated query
  // (below) so they actually constrain the result.
  if (filters.length > 0) params.set('filters', encodeFilters(filters));
  const filterClauses = filters.map((f) => fieldClause(f.key, f.value, f.operator === '!=' ? '!=' : '='));
  const withFilters = (query: string) => [query, ...filterClauses].filter(Boolean).join(' AND ');
  const context = buildSignalContext(type, value, options);
  appendExplicitContext(params, context);

  if (type === 'trace_id') {
    const traceParams = paramsWith(params, 'q', withFilters(fieldClause('trace_id', context.traceId ?? value)));
    const logsParams = paramsWith(params, 'q', withFilters(logFieldQuery(context)));
    const metricsParams = paramsWith(params, 'promql', metricQueryForContext(context, filters));
    return [
      {
        id: 'traces',
        labelKey: 'signal_reference.jumps.open_trace',
        to: `/traces?${traceParams}`,
        relation: 'exact',
      },
      {
        id: 'logs',
        labelKey: 'signal_reference.jumps.trace_logs',
        to: `/logs?${logsParams}`,
        relation: 'exact',
      },
      {
        id: 'metrics',
        labelKey: 'signal_reference.jumps.service_metrics',
        to: `/metrics?${metricsParams}`,
        relation: 'context',
      },
    ];
  }
  if (type === 'span_id') {
    const similarContext = withoutIdentifiers(context);
    const logsParams = paramsWith(params, 'q', withFilters(logFieldQuery(context)));
    const metricsParams = paramsWith(params, 'promql', metricQueryForContext(similarContext, filters));
    const tracesParams = paramsWith(params, 'q', withFilters(traceFieldQuery(similarContext)));
    return [
      {
        id: 'logs',
        labelKey: 'signal_reference.jumps.span_logs',
        to: `/logs?${logsParams}`,
        relation: 'exact',
      },
      {
        id: 'metrics',
        labelKey: 'signal_reference.jumps.service_metrics',
        to: `/metrics?${metricsParams}`,
        relation: 'context',
      },
      {
        id: 'traces',
        labelKey: 'signal_reference.jumps.similar_traces',
        to: `/traces?${tracesParams}`,
        relation: 'context',
      },
    ];
  }
  if (type === 'service') {
    const serviceContext = serviceOnlyContext(context);
    const metricsParams = paramsWith(params, 'promql', metricQueryForContext(serviceContext, filters));
    const logsParams = paramsWith(params, 'q', withFilters(logFieldQuery(serviceContext)));
    const tracesParams = paramsWith(params, 'q', withFilters(traceFieldQuery(serviceContext)));
    return [
      {
        id: 'metrics',
        labelKey: 'signal_reference.jumps.service_metrics',
        to: `/metrics?${metricsParams}`,
        relation: 'context',
      },
      {
        id: 'traces',
        labelKey: 'signal_reference.jumps.service_traces',
        to: `/traces?${tracesParams}`,
        relation: 'context',
      },
      {
        id: 'logs',
        labelKey: 'signal_reference.jumps.service_logs',
        to: `/logs?${logsParams}`,
        relation: 'context',
      },
    ];
  }
  if (type === 'host') {
    const metricsParams = paramsWith(params, 'promql', metricQueryForContext(context, filters));
    const logsParams = paramsWith(params, 'q', withFilters(logFieldQuery(context)));
    const tracesParams = paramsWith(params, 'q', withFilters(traceFieldQuery(context)));
    return [
      {
        id: 'metrics',
        labelKey: 'signal_reference.jumps.host_metrics',
        to: `/metrics?${metricsParams}`,
        relation: 'context',
      },
      {
        id: 'traces',
        labelKey: context.service
          ? 'signal_reference.jumps.service_traces'
          : 'signal_reference.jumps.open_traces',
        to: `/traces?${tracesParams}`,
        relation: 'context',
      },
      {
        id: 'logs',
        labelKey: 'signal_reference.jumps.matching_logs',
        to: `/logs?${logsParams}`,
        relation: 'context',
      },
    ];
  }
  // stream
  const streamType = options.streamType ?? 'logs';
  const openTarget = options.streamId ?? value;
  const streamParams = new URLSearchParams(params);
  if (streamType === 'metrics') {
    streamParams.set('metric', value);
    return [
      {
        id: 'stream',
        labelKey: 'signal_reference.jumps.open_stream',
        to: `/streams/${encodeURIComponent(openTarget)}`,
        relation: 'exact',
      },
      {
        id: 'metrics',
        labelKey: 'signal_reference.jumps.stream_metrics',
        to: `/metrics?${streamParams}`,
        relation: 'exact',
      },
    ];
  }
  if (streamType === 'traces') {
    streamParams.set('stream', value);
    return [
      {
        id: 'stream',
        labelKey: 'signal_reference.jumps.open_stream',
        to: `/streams/${encodeURIComponent(openTarget)}`,
        relation: 'exact',
      },
      {
        id: 'traces',
        labelKey: 'signal_reference.jumps.stream_traces',
        to: `/traces?${streamParams}`,
        relation: 'exact',
      },
    ];
  }
  if (streamType === 'profiles') {
    return [
      {
        id: 'stream',
        labelKey: 'signal_reference.jumps.open_stream',
        to: `/streams/${encodeURIComponent(openTarget)}`,
        relation: 'exact',
      },
      {
        id: 'stream',
        labelKey: 'signal_reference.jumps.stream_profiles',
        to: '/profiles',
        relation: 'context',
      },
    ];
  }
  if (streamType === 'extend') {
    return [
      {
        id: 'stream',
        labelKey: 'signal_reference.jumps.open_stream',
        to: `/streams/${encodeURIComponent(openTarget)}`,
        relation: 'exact',
      },
    ];
  }
  streamParams.set('stream', value);
  return [
    {
      id: 'stream',
      labelKey: 'signal_reference.jumps.open_stream',
      to: `/streams/${encodeURIComponent(openTarget)}`,
      relation: 'exact',
    },
    {
      id: 'logs',
      labelKey: 'signal_reference.jumps.stream_logs',
      to: `/logs?${streamParams}`,
      relation: 'exact',
    },
  ];
}

interface SignalContext {
  type: SignalReferenceType;
  value: string;
  labelName?: string | undefined;
  labels: Record<string, string>;
  metricQuery?: string | undefined;
  traceId?: string | undefined;
  spanId?: string | undefined;
  service?: string | undefined;
  host?: string | undefined;
  hostLabelName?: string | undefined;
  route?: string | undefined;
  method?: string | undefined;
  status?: string | undefined;
  operation?: string | undefined;
  environment?: string | undefined;
  environmentLabelName?: string | undefined;
}

const ROUTE_LABEL_ALIASES = ['route', 'path', 'http.route', 'url_path'];
const METHOD_LABEL_ALIASES = ['method', 'http.method'];
const STATUS_LABEL_ALIASES = ['status_code', 'status', 'http.status_code'];
const OPERATION_LABEL_ALIASES = ['operation_name', 'operation', 'span.name'];
const ENVIRONMENT_LABEL_ALIASES = [
  'environment',
  'env',
  'deployment.environment',
  'deployment.environment.name',
  'deployment_environment',
];

function buildSignalContext(
  type: SignalReferenceType,
  value: string,
  options: { labelName?: string | undefined; labels?: Record<string, string> | undefined; metricQuery?: string | undefined },
): SignalContext {
  const labels = options.labels ?? {};
  const hostHit = findLabeledValue(labels, HOST_LABEL_ALIASES);
  const environmentHit = findLabeledValue(labels, ENVIRONMENT_LABEL_ALIASES);
  return {
    type,
    value,
    labelName: options.labelName,
    labels,
    metricQuery: options.metricQuery,
    traceId: findLabelValue(labels, TRACE_ID_LABEL_ALIASES) ?? (type === 'trace_id' ? value : undefined),
    spanId: findLabelValue(labels, SPAN_ID_LABEL_ALIASES) ?? (type === 'span_id' ? value : undefined),
    service: findLabelValue(labels, SERVICE_LABEL_ALIASES) ?? (type === 'service' ? value : undefined),
    host: hostHit?.value ?? (type === 'host' ? value : undefined),
    hostLabelName: hostHit?.key ?? (type === 'host' ? options.labelName : undefined),
    route: findLabelValue(labels, ROUTE_LABEL_ALIASES),
    method: findLabelValue(labels, METHOD_LABEL_ALIASES),
    status: findLabelValue(labels, STATUS_LABEL_ALIASES),
    operation: findLabelValue(labels, OPERATION_LABEL_ALIASES),
    environment: environmentHit?.value,
    environmentLabelName: environmentHit?.key,
  };
}

function appendExplicitContext(params: URLSearchParams, context: SignalContext): void {
  if (context.type === 'trace_id' || context.type === 'span_id') {
    if (context.traceId) params.set('trace_id', context.traceId);
    if (context.spanId) params.set('span_id', context.spanId);
  }
  if (context.service) params.set('service', context.service);
  if (context.operation && context.type !== 'service') params.set('operation', context.operation);
  if (context.environment) params.set('environment', context.environment);
}

function withoutIdentifiers(context: SignalContext): SignalContext {
  return { ...context, traceId: undefined, spanId: undefined };
}

function serviceOnlyContext(context: SignalContext): SignalContext {
  return {
    ...withoutIdentifiers(context),
    route: undefined,
    method: undefined,
    status: undefined,
    operation: undefined,
    host: undefined,
    hostLabelName: undefined,
  };
}

function findLabelValue(labels: Record<string, string>, aliases: string[]): string | undefined {
  return findLabeledValue(labels, aliases)?.value;
}

function findLabeledValue(labels: Record<string, string>, aliases: string[]): { key: string; value: string } | undefined {
  const normalizedAliases = aliases.map((alias) => alias.toLowerCase());
  for (const [key, value] of Object.entries(labels)) {
    if (normalizedAliases.includes(key.toLowerCase()) && value) return { key, value };
  }
  return undefined;
}

function paramsWith(base: URLSearchParams, key: string, value: string): URLSearchParams {
  const params = new URLSearchParams(base);
  if (value) params.set(key, value);
  return params;
}

function quoteFieldValue(value: string): string {
  return value.replace(/\\/g, '\\\\').replace(/'/g, "\\'");
}

function fieldClause(field: string, value: string, op: '=' | '!=' | 'contains' = '='): string {
  return `${field} ${op} '${quoteFieldValue(value)}'`;
}

function logFieldQuery(context: SignalContext): string {
  const clauses: string[] = [];
  if (context.traceId) {
    clauses.push(fieldClause('trace_id', context.traceId));
    if (context.spanId) clauses.push(fieldClause('span_id', context.spanId));
    return clauses.join(' AND ');
  }
  if (context.service) clauses.push(fieldClause('service', context.service));
  if (context.route) clauses.push(fieldClause('path', context.route));
  else if (context.operation) clauses.push(fieldClause('operation', context.operation));
  if (context.method) clauses.push(fieldClause('method', context.method));
  if (context.status && /^\d+$/.test(context.status)) clauses.push(fieldClause('status_code', context.status));
  if (context.host && context.hostLabelName?.toLowerCase() !== 'instance') clauses.push(fieldClause('host', context.host));
  if (context.environment) {
    clauses.push(fieldClause(context.environmentLabelName ?? 'environment', context.environment));
  }
  if (clauses.length > 0) return clauses.join(' AND ');
  if (context.type === 'host') return fieldClause('host', context.host ?? context.value);
  if (context.type === 'service') return fieldClause('service', context.service ?? context.value);
  return '';
}

function traceFieldQuery(context: SignalContext): string {
  const clauses: string[] = [];
  if (context.traceId) clauses.push(fieldClause('trace_id', context.traceId));
  if (context.spanId) clauses.push(fieldClause('span_id', context.spanId));
  if (context.service) clauses.push(fieldClause('service_name', context.service));
  const operation = context.operation ?? context.route;
  if (operation && !context.traceId) clauses.push(fieldClause('operation_name', operation, 'contains'));
  return clauses.join(' AND ');
}

function metricQueryForContext(context: SignalContext, filters: GlobalFilter[] = []): string {
  const matchers = metricMatchers(context, filters);
  const labelExpr = metricMatcherExpression(matchers);
  const query = context.metricQuery?.trim();
  if (query) {
    const rateMatch = query.match(/^rate\(\s*([a-zA-Z_:][\w:]*)(?:\{[^}]*\})?\s*\[([^\]]+)\]\s*\)$/);
    if (rateMatch) return `rate(${rateMatch[1]}${labelExpr}[${rateMatch[2]}])`;
    const bareMetricMatch = query.match(/^([a-zA-Z_:][\w:]*)(?:\{[^}]*\})?$/);
    if (bareMetricMatch) return `${bareMetricMatch[1]}${labelExpr}`;
  }
  const metricName = findLabelValue(context.labels, ['__name__', 'metric', 'metric_name']) ?? 'http_requests_total';
  return `rate(${metricName}${labelExpr}[5m])`;
}

interface MetricMatcher {
  value: string;
  operator: '=' | '!=';
}

function metricMatchers(context: SignalContext, filters: GlobalFilter[] = []): Record<string, MetricMatcher> {
  const pinned = Object.fromEntries(
    filters.map((filter) => [
      filter.key,
      { value: filter.value, operator: filter.operator === '!=' ? '!=' as const : '=' as const },
    ]),
  );
  if (context.metricQuery) {
    const fromLabels = Object.fromEntries(
      Object.entries(context.labels)
        .filter(([key, value]) => isPrometheusLabel(key) && value)
        .map(([key, value]) => [key, { value, operator: '=' as const }]),
    );
    if (Object.keys(fromLabels).length > 0) return { ...fromLabels, ...pinned };
  }
  const matchers: Record<string, MetricMatcher> = {};
  const exact = (value: string): MetricMatcher => ({ value, operator: '=' });
  if (context.service) matchers.service = exact(context.service);
  if (context.route) matchers.route = exact(context.route);
  else if (context.operation) matchers.operation = exact(context.operation);
  if (context.method) matchers.method = exact(context.method);
  if (context.status) matchers.status = exact(context.status);
  if (context.host && context.hostLabelName?.toLowerCase() !== 'instance') {
    matchers.host = exact(context.host);
  }
  if (context.environment) {
    const environmentKey =
      context.environmentLabelName && isPrometheusLabel(context.environmentLabelName)
        ? context.environmentLabelName
        : 'environment';
    matchers[environmentKey] = exact(context.environment);
  }
  return { ...matchers, ...pinned };
}

function isPrometheusLabel(name: string): boolean {
  return /^[a-zA-Z_][a-zA-Z0-9_]*$/.test(name) && name !== '_timestamp' && name !== 'timestamp' && name !== 'time';
}

function metricMatcherExpression(matchers: Record<string, MetricMatcher>): string {
  const entries = Object.entries(matchers).filter(([key]) => isPrometheusLabel(key));
  if (entries.length === 0) return '';
  return `{${entries.map(([key, matcher]) => (
    `${key}${matcher.operator}"${matcher.value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`
  )).join(',')}}`;
}
