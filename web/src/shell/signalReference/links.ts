import { encodeFilters } from '@/shell/UrlHydration';
import type { GlobalFilter } from '@/stores/useFiltersStore';

export type SignalReferenceType = 'trace_id' | 'span_id' | 'service' | 'host' | 'stream';
export type SignalReferenceStreamType = 'logs' | 'metrics' | 'traces' | 'profiles' | 'extend';

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

export type SignalJumpKind = 'traces' | 'logs' | 'metrics' | 'stream';

export interface SignalJumpAction {
  id: SignalJumpKind;
  labelKey: string;
  to: string;
  relation: 'exact' | 'context';
}

export interface SignalContext {
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

const TRACE_ID_LABEL_ALIASES = ['trace_id', 'traceid', 'trace.id'];
const SPAN_ID_LABEL_ALIASES = ['span_id', 'spanid', 'span.id'];
const SERVICE_LABEL_ALIASES = ['service', 'service_name', 'service.name', 'svc'];
const HOST_LABEL_ALIASES = ['host', 'host_name', 'hostname', 'host.name', 'node', 'instance'];
const STREAM_LABEL_ALIASES = ['stream', 'stream_name'];
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

export function detectSignalTypeForLabel(label: string): SignalReferenceType | null {
  const lower = label.toLowerCase();
  if (TRACE_ID_LABEL_ALIASES.includes(lower)) return 'trace_id';
  if (SPAN_ID_LABEL_ALIASES.includes(lower)) return 'span_id';
  if (SERVICE_LABEL_ALIASES.includes(lower)) return 'service';
  if (HOST_LABEL_ALIASES.includes(lower)) return 'host';
  if (STREAM_LABEL_ALIASES.includes(lower)) return 'stream';
  return null;
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
  if (filters.length > 0) params.set('filters', encodeFilters(filters));
  const filterClauses = filters.map((filter) =>
    fieldClause(filter.key, filter.value, filter.operator === '!=' ? '!=' : '='),
  );
  const withFilters = (query: string) =>
    [query, ...filterClauses].filter(Boolean).join(' AND ');
  const context = buildSignalContext(type, value, options);
  appendExplicitContext(params, context);

  if (type === 'trace_id') {
    const traceId = context.traceId ?? value;
    const logsParams = paramsWith(params, 'q', withFilters(logFieldQuery(context)));
    const metricsParams = paramsWith(params, 'promql', metricQueryForContext(context, filters));
    return [
      {
        id: 'traces',
        labelKey: 'signal_reference.jumps.open_trace',
        to: exactTracePath(traceId, params),
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
      context.traceId
        ? {
            id: 'traces',
            labelKey: 'signal_reference.jumps.open_trace',
            to: exactTracePath(context.traceId, params),
            relation: 'exact',
          }
        : {
            id: 'traces',
            labelKey: 'signal_reference.jumps.similar_traces',
            to: `/traces?${tracesParams}`,
            relation: 'context',
          },
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
    ];
  }

  if (type === 'service') {
    const serviceContext = serviceOnlyContext(context);
    return contextSignalJumps(serviceContext, params, filters, withFilters, {
      metrics: 'signal_reference.jumps.service_metrics',
      traces: 'signal_reference.jumps.service_traces',
      logs: 'signal_reference.jumps.service_logs',
    });
  }

  if (type === 'host') {
    return contextSignalJumps(context, params, filters, withFilters, {
      metrics: 'signal_reference.jumps.host_metrics',
      traces: context.service
        ? 'signal_reference.jumps.service_traces'
        : 'signal_reference.jumps.open_traces',
      logs: 'signal_reference.jumps.matching_logs',
    });
  }

  return streamSignalJumps(value, params, options);
}

export function buildSignalContext(
  type: SignalReferenceType,
  value: string,
  options: Pick<SignalReferenceOptions, 'labelName' | 'labels' | 'metricQuery'>,
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

function contextSignalJumps(
  context: SignalContext,
  params: URLSearchParams,
  filters: GlobalFilter[],
  withFilters: (query: string) => string,
  labels: { metrics: string; traces: string; logs: string },
): SignalJumpAction[] {
  const metricsParams = paramsWith(params, 'promql', metricQueryForContext(context, filters));
  const logsParams = paramsWith(params, 'q', withFilters(logFieldQuery(context)));
  const tracesParams = paramsWith(params, 'q', withFilters(traceFieldQuery(context)));
  return [
    { id: 'metrics', labelKey: labels.metrics, to: `/metrics?${metricsParams}`, relation: 'context' },
    { id: 'traces', labelKey: labels.traces, to: `/traces?${tracesParams}`, relation: 'context' },
    { id: 'logs', labelKey: labels.logs, to: `/logs?${logsParams}`, relation: 'context' },
  ];
}

function streamSignalJumps(
  value: string,
  params: URLSearchParams,
  options: SignalReferenceOptions,
): SignalJumpAction[] {
  const streamType = options.streamType ?? 'logs';
  const openTarget = options.streamId ?? value;
  const openStream: SignalJumpAction = {
    id: 'stream',
    labelKey: 'signal_reference.jumps.open_stream',
    to: `/streams/${encodeURIComponent(openTarget)}`,
    relation: 'exact',
  };
  const streamParams = new URLSearchParams(params);
  if (streamType === 'metrics') {
    streamParams.set('metric', value);
    return [openStream, {
      id: 'metrics',
      labelKey: 'signal_reference.jumps.stream_metrics',
      to: `/metrics?${streamParams}`,
      relation: 'exact',
    }];
  }
  if (streamType === 'traces') {
    streamParams.set('stream', value);
    return [openStream, {
      id: 'traces',
      labelKey: 'signal_reference.jumps.stream_traces',
      to: `/traces?${streamParams}`,
      relation: 'exact',
    }];
  }
  if (streamType === 'profiles') {
    return [openStream, {
      id: 'stream',
      labelKey: 'signal_reference.jumps.stream_profiles',
      to: '/profiles',
      relation: 'context',
    }];
  }
  if (streamType === 'extend') return [openStream];
  streamParams.set('stream', value);
  return [openStream, {
    id: 'logs',
    labelKey: 'signal_reference.jumps.stream_logs',
    to: `/logs?${streamParams}`,
    relation: 'exact',
  }];
}

function exactTracePath(traceId: string, base: URLSearchParams): string {
  const params = new URLSearchParams(base);
  params.delete('q');
  const query = params.toString();
  return `/traces/${encodeURIComponent(traceId)}${query ? `?${query}` : ''}`;
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

function findLabeledValue(
  labels: Record<string, string>,
  aliases: string[],
): { key: string; value: string } | undefined {
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

function fieldClause(
  field: string,
  value: string,
  op: '=' | '!=' | 'contains' = '=',
): string {
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
  if (context.status && /^\d+$/.test(context.status)) {
    clauses.push(fieldClause('status_code', context.status));
  }
  if (context.host && context.hostLabelName?.toLowerCase() !== 'instance') {
    clauses.push(fieldClause('host', context.host));
  }
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
  if (operation && !context.traceId) {
    clauses.push(fieldClause('operation_name', operation, 'contains'));
  }
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
  const metricName =
    findLabelValue(context.labels, ['__name__', 'metric', 'metric_name']) ??
    'http_requests_total';
  return `rate(${metricName}${labelExpr}[5m])`;
}

interface MetricMatcher {
  value: string;
  operator: '=' | '!=';
}

function metricMatchers(
  context: SignalContext,
  filters: GlobalFilter[] = [],
): Record<string, MetricMatcher> {
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
  return (
    /^[a-zA-Z_][a-zA-Z0-9_]*$/.test(name) &&
    name !== '_timestamp' &&
    name !== 'timestamp' &&
    name !== 'time'
  );
}

function metricMatcherExpression(matchers: Record<string, MetricMatcher>): string {
  const entries = Object.entries(matchers).filter(([key]) => isPrometheusLabel(key));
  if (entries.length === 0) return '';
  return `{${entries
    .map(
      ([key, matcher]) =>
        `${key}${matcher.operator}"${matcher.value
          .replace(/\\/g, '\\\\')
          .replace(/"/g, '\\"')}"`,
    )
    .join(',')}}`;
}
