import { keepPreviousData, useQueries } from '@tanstack/react-query';
import { ChevronDown } from 'lucide-react';
import * as React from 'react';

import * as dashboardsApi from '@/api/dashboards';
import { ChromeButton } from '@/shell/chrome';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

import { useDashboardText } from '../i18n';
import {
  resolveRefreshIntervalMilliseconds,
  type DashboardRefreshCadence,
  type DashboardTimeRangeResolver,
} from '../refresh/policy';
import type { DashboardTimeRange, DashboardVariable } from '../schema';
import {
  interpolateVariables,
  variableOptions,
  type DashboardVariableValues,
} from '../variables';

/**
 * The dashboard's variables as controls above its panels. Query variables
 * resolve their options here, for the time range the dashboard is showing.
 */
export function DashboardVariableBar({
  dashboardUid,
  variables,
  values,
  timeRange,
  timeRangeKey,
  resolveTimeRange,
  refreshCadence,
  containerWidth,
  onChange,
  disabled,
  resolveQueries,
}: {
  dashboardUid: string;
  variables: readonly DashboardVariable[];
  values: DashboardVariableValues;
  timeRange: DashboardTimeRange;
  timeRangeKey: string;
  resolveTimeRange: DashboardTimeRangeResolver;
  refreshCadence: DashboardRefreshCadence;
  containerWidth: number;
  onChange: (values: DashboardVariableValues) => void;
  disabled: boolean;
  resolveQueries: boolean;
}) {
  const queryVariables = variables.filter(
    (variable) => variable.type === 'query',
  );
  const refreshInterval = resolveRefreshIntervalMilliseconds(
    refreshCadence,
    timeRange,
    containerWidth,
  );
  const queries = useQueries({
    queries: queryVariables.map((variable) => {
      const query = variable.query ?? {};
      const expression = interpolateVariables(
        stringValue(query.expression ?? query.query),
        values,
      );
      const kind = query.kind === 'sql' ? 'sql' : 'query';
      const streamName = stringValue(query.streamName ?? query.stream);
      const streamType = stringValue(query.streamType ?? query.stream_type);
      return {
        queryKey: [
          'dashboard-engine-variable',
          dashboardUid,
          variable.id,
          expression,
          timeRangeKey,
        ],
        queryFn: () => {
          const queryTimeRange = resolveTimeRange();
          return dashboardsApi.resolveVariable({
            variable: {
              name: variable.name,
              query: expression,
              kind,
              ...(streamName
                ? {
                    stream: {
                      name: streamName,
                      ...(streamType ? { streamType: streamType } : {}),
                    },
                  }
                : {}),
            },
            timeRange: {
              start: queryTimeRange.from,
              end: queryTimeRange.to,
            },
            limit: 500,
          });
        },
        enabled: resolveQueries && Boolean(expression),
        staleTime: 30_000,
        placeholderData: keepPreviousData,
        refetchInterval: refreshInterval,
      };
    }),
  });
  const resolved = new Map(
    queryVariables.map((variable, index) => [
      variable.id,
      queries[index]?.data?.values ?? [],
    ]),
  );
  // A person who has not chosen is given the first of what there is. A choice that is not among the
  // options stays: it may come from a link, for a range that does not hold it.
  const unchosen = queryVariables.flatMap((variable) => {
    const options = resolved.get(variable.id) ?? [];
    const value = values[variable.name];
    const empty = value === undefined || value === null || value === '' || (Array.isArray(value) && value.length === 0);
    return !variable.multi && empty && options.length > 0 ? [[variable.name, options[0]] as const] : [];
  });
  const unchosenKey = JSON.stringify(unchosen);
  React.useEffect(() => {
    if (unchosen.length === 0 || disabled) return;
    onChange({ ...values, ...Object.fromEntries(unchosen) });
    // The first options are keyed by their text, as `unchosen` is a new list on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [unchosenKey, disabled]);
  const visible = variables.filter(
    (variable) =>
      variable.hide !== 'variable' &&
      variable.type !== 'constant',
  );
  if (visible.length === 0) return null;

  return (
    <div className="flex flex-wrap items-end gap-2 border-b border-bd-0 pb-3">
      {visible.map((variable) => {
        const resolvedValues = resolved.get(variable.id);
        const listed =
          resolvedValues && resolvedValues.length > 0
            ? resolvedValues.map((value) => ({ label: value, value }))
            : variableOptions(variable);
        const options = withChosen(listed, values[variable.name], variable);
        return (
          <VariableControl
            key={variable.id}
            variable={variable}
            options={options}
            value={values[variable.name]}
            disabled={disabled}
            onChange={(value) =>
              onChange({ ...values, [variable.name]: value })
            }
          />
        );
      })}
    </div>
  );
}

function VariableControl({
  variable,
  options,
  value,
  onChange,
  disabled,
}: {
  variable: DashboardVariable;
  options: Array<{ label: string; value: unknown }>;
  value: unknown;
  onChange: (value: unknown) => void;
  disabled: boolean;
}) {
  const tr = useDashboardText();
  if (variable.type === 'text') {
    return (
      <label className="grid gap-1 font-sans text-type-micro text-tx-3">
        {variable.hide === 'label' ? null : variable.label}
        <input
          disabled={disabled}
          value={String(value ?? '')}
          onChange={(event) => onChange(event.target.value)}
          className="h-8 min-w-36 rounded-md border border-bd-1 bg-[var(--control-surface)] px-2 font-mono text-xs text-tx-1 outline-none focus-visible:bg-bg-3 disabled:cursor-not-allowed disabled:opacity-60"
        />
      </label>
    );
  }
  if (variable.multi) {
    const selected = Array.isArray(value) ? value : value === undefined ? [] : [value];
    return (
      <div className="grid gap-1 font-sans text-type-micro text-tx-3">
        {variable.hide === 'label' ? null : variable.label}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <ChromeButton
              disabled={disabled}
              className="min-w-36 justify-between"
            >
              <span className="max-w-48 truncate">
                {selected.length === 0
                  ? tr('Select')
                  : selected.map(String).join(', ')}
              </span>
              <ChevronDown className="h-3 w-3" />
            </ChromeButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent className="max-h-72 min-w-56 overflow-auto">
            <DropdownMenuLabel>{variable.label}</DropdownMenuLabel>
            {options.map((option) => {
              const checked = selected.some((entry) =>
                Object.is(entry, option.value),
              );
              return (
                <DropdownMenuCheckboxItem
                  key={`${option.label}-${String(option.value)}`}
                  checked={checked}
                  disabled={disabled}
                  onSelect={(event) => event.preventDefault()}
                  onCheckedChange={(next) =>
                    onChange(
                      next
                        ? [...selected, option.value]
                        : selected.filter(
                            (entry) => !Object.is(entry, option.value),
                          ),
                    )
                  }
                >
                  {option.label}
                </DropdownMenuCheckboxItem>
              );
            })}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    );
  }
  return (
    <label className="grid gap-1 font-sans text-type-micro text-tx-3">
      {variable.hide === 'label' ? null : variable.label}
      <select
        disabled={disabled}
        value={String(value ?? '')}
        onChange={(event) => {
          const option = options.find(
            (candidate) => String(candidate.value) === event.target.value,
          );
          onChange(option?.value ?? event.target.value);
        }}
        className="h-8 min-w-36 rounded-md border border-bd-1 bg-[var(--control-surface)] px-2 font-sans text-xs text-tx-1 outline-none focus-visible:bg-bg-3 disabled:cursor-not-allowed disabled:opacity-60"
      >
        {options.map((option) => (
          <option
            key={`${option.label}-${String(option.value)}`}
            value={String(option.value)}
          >
            {option.label}
          </option>
        ))}
      </select>
    </label>
  );
}

/**
 * What was chosen is always among the options, so the control shows it: a value from the address
 * need not be one of the values a range holds.
 */
function withChosen(
  options: Array<{ label: string; value: unknown }>,
  chosen: unknown,
  variable: DashboardVariable,
): Array<{ label: string; value: unknown }> {
  if (variable.multi || variable.type === 'text') return options;
  if (typeof chosen !== 'string' && typeof chosen !== 'number') return options;
  if (chosen === '') return options;
  if (options.some((option) => String(option.value) === String(chosen))) return options;
  return [{ label: String(chosen), value: chosen }, ...options];
}

function stringValue(value: unknown): string {
  return typeof value === 'string' ? value : '';
}
