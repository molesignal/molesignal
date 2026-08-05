import type { DashboardTimeRange, DataLink } from './schema';
import {
  interpolateVariables,
  type DashboardVariableValues,
} from './variables';

export interface DataLinkContext {
  variables: DashboardVariableValues;
  timeRange: DashboardTimeRange;
  field?: {
    name?: string;
    value?: unknown;
    labels?: Record<string, string>;
  };
}

const TARGET_ROUTES: Record<
  Exclude<DataLink['target'], 'external' | 'dashboard'>,
  string
> = {
  logs: '/logs',
  metrics: '/metrics',
  traces: '/traces',
  profiles: '/profiles',
};

export function buildDataLinkUrl(
  link: DataLink,
  context: DataLinkContext,
): string {
  const interpolationValues = {
    ...context.variables,
    '__field.name': context.field?.name ?? '',
    '__field.value': context.field?.value ?? '',
    ...Object.fromEntries(
      Object.entries(context.field?.labels ?? {}).map(([name, value]) => [
        `__field.labels.${name}`,
        value,
      ]),
    ),
  };
  const rawBase =
    link.url ??
    (link.target === 'dashboard'
      ? '/dashboards'
      : link.target === 'external'
        ? ''
        : TARGET_ROUTES[link.target]);
  const base = interpolateExtended(rawBase, interpolationValues);
  if (!base) return '';

  const origin =
    typeof window === 'undefined' ? 'http://dashboard.local' : window.location.origin;
  const url = new URL(base, origin);
  for (const [name, value] of Object.entries(link.variables)) {
    url.searchParams.set(name, interpolateExtended(value, interpolationValues));
  }
  if (link.includeTimeRange) {
    url.searchParams.set('from', String(context.timeRange.from));
    url.searchParams.set('to', String(context.timeRange.to));
  }
  if (link.includeDashboardVariables) {
    for (const [name, value] of Object.entries(context.variables)) {
      url.searchParams.set(`var-${name}`, stringify(value));
    }
  }
  return url.origin === origin
    ? `${url.pathname}${url.search}${url.hash}`
    : url.toString();
}

function interpolateExtended(
  input: string,
  values: DashboardVariableValues,
): string {
  const withFieldContext = input.replace(
    /\$\{(__field(?:\.labels)?\.[A-Za-z_][A-Za-z0-9_.-]*)\}/g,
    (match, name: string) =>
      name in values ? stringify(values[name]) : match,
  );
  return interpolateVariables(withFieldContext, values);
}

function stringify(value: unknown): string {
  return Array.isArray(value)
    ? value.map((entry) => stringify(entry)).join(',')
    : String(value ?? '');
}
