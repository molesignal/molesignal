import type { TFunction } from 'i18next';
import { AlertTriangle, Plus, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { MetricCatalogEntry } from '@/api/metricsCatalog';
import type { PromqlCapabilities } from '@/api/query';
import { resolveMetricType } from '@/lib/metricTypes';
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

import {
  BUILDER_AGGREGATIONS,
  BUILDER_RANGES,
  builderFunctionOptions,
  composeBuilderPromql,
  emptyBuilderQuery,
  newBuilderMatcher,
  parseBuilderPromql,
  type BuilderAggregation,
  type BuilderFunctionOption,
  type BuilderMatcher,
  type BuilderMatcherOperator,
  type BuilderQuery,
  type BuilderTransform,
} from './model';

interface PromqlBuilderPanelProps {
  expression: string;
  metrics: MetricCatalogEntry[];
  pending: boolean;
  error: unknown;
  capabilities: PromqlCapabilities | undefined;
  onExpressionChange: (expression: string) => void;
}

export function PromqlBuilderPanel({
  expression,
  metrics,
  pending,
  error,
  capabilities,
  onExpressionChange,
}: PromqlBuilderPanelProps) {
  const { t } = useTranslation('metrics');
  const functionOptions = React.useMemo(
    () => builderFunctionOptions(capabilities?.functions),
    [capabilities?.functions],
  );
  const [query, setQuery] = React.useState<BuilderQuery>(
    () => parseBuilderPromql(expression, functionOptions) ?? emptyBuilderQuery(),
  );
  const [unsupportedExpression, setUnsupportedExpression] = React.useState(
    () => parseBuilderPromql(expression, functionOptions) === null,
  );
  const lastComposedExpression = React.useRef<string | null>(null);

  React.useEffect(() => {
    if (expression === lastComposedExpression.current) return;
    const parsed = parseBuilderPromql(expression, functionOptions);
    setUnsupportedExpression(parsed === null);
    if (parsed) setQuery(parsed);
  }, [expression, functionOptions]);

  const metricOptions = React.useMemo(() => {
    const byName = new Map(metrics.map((metric) => [metric.name, metric]));
    if (query.metric && !byName.has(query.metric)) {
      byName.set(query.metric, {
        name: query.metric,
        labels: query.matchers.map((matcher) => matcher.name).filter(Boolean),
        field_count: 0,
      });
    }
    return [...byName.values()].sort((left, right) =>
      left.name.localeCompare(right.name),
    );
  }, [metrics, query.matchers, query.metric]);
  const selectedMetric = metricOptions.find(
    (metric) => metric.name === query.metric,
  );
  const availableLabels = selectedMetric?.labels ?? [];
  const selectedFunction = functionOptions.find(
    (option) => option.name === query.transform,
  );
  const supportedFunctions = functionOptions.filter(
    (option) => option.name === 'none' || option.input !== null,
  );
  const codeOnlyFunctions = functionOptions.filter(
    (option) => option.input === null,
  );
  const composedExpression = composeBuilderPromql(query, functionOptions);

  const updateQuery = React.useCallback(
    (next: BuilderQuery) => {
      setQuery(next);
      setUnsupportedExpression(false);
      const composed = composeBuilderPromql(next, functionOptions);
      lastComposedExpression.current = composed;
      onExpressionChange(composed);
    },
    [functionOptions, onExpressionChange],
  );

  const chooseMetric = (metricName: string) => {
    const metric = metrics.find((item) => item.name === metricName);
    const nextTransform = metric
      ? resolveMetricType(metric) === 'counter'
        ? 'rate'
        : 'none'
      : query.transform;
    updateQuery({
      ...query,
      metric: metricName,
      transform: nextTransform,
      matchers: query.matchers.filter((matcher) =>
        metric?.labels.includes(matcher.name),
      ),
    });
  };

  const updateMatcher = (index: number, matcher: BuilderMatcher) => {
    const matchers = [...query.matchers];
    matchers[index] = matcher;
    updateQuery({ ...query, matchers });
  };

  return (
    <div data-testid="promql-builder-panel" className="bg-bg-1">
      {unsupportedExpression ? (
        <div className="flex items-start gap-2 border-b border-yellow-dim bg-yellow-dim/15 px-3 py-2 font-sans text-xs text-yellow-soft">
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <span>{t('builder.unsupported_expression')}</span>
        </div>
      ) : null}

      <div className="grid grid-cols-1 gap-3 p-3 md:grid-cols-2 xl:grid-cols-[minmax(260px,2fr)_minmax(150px,1fr)_minmax(150px,1fr)_minmax(130px,0.8fr)]">
        <BuilderField label={t('builder.metric_label')}>
          <Select
            {...(query.metric ? { value: query.metric } : {})}
            onValueChange={chooseMetric}
          >
            <SelectTrigger
              aria-label={t('builder.metric_select_aria')}
              className="h-11 rounded-md border-bd-1 bg-bg-2 text-base sm:h-9 sm:text-sm"
              disabled={pending || Boolean(error)}
            >
              <SelectValue
                placeholder={
                  pending
                    ? t('explore.catalog.loading')
                    : t('builder.metric_select_placeholder')
                }
              />
            </SelectTrigger>
            <SelectContent className="max-w-[min(680px,calc(100vw-32px))]">
              {metricOptions.map((metric) => (
                <SelectItem
                  key={metric.name}
                  value={metric.name}
                  className="font-mono text-xs"
                >
                  {metric.name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </BuilderField>

        <BuilderField label={t('builder.function')}>
          <Select
            value={query.transform}
            onValueChange={(value) =>
              updateQuery({ ...query, transform: value as BuilderTransform })
            }
          >
            <SelectTrigger
              aria-label={t('builder.function_select_aria')}
              className="h-11 rounded-md border-bd-1 bg-bg-2 text-base sm:h-9 sm:text-sm"
            >
              <SelectValue>
                {builderFunctionLabel(query.transform, t)}
              </SelectValue>
            </SelectTrigger>
            <SelectContent className="min-w-[min(420px,calc(100vw-32px))]">
              <SelectGroup>
                <SelectLabel>{t('builder.available_functions')}</SelectLabel>
                {supportedFunctions.map((option) => (
                  <FunctionSelectItem
                    key={option.name}
                    option={option}
                    label={builderFunctionLabel(option.name, t)}
                  />
                ))}
              </SelectGroup>
              {codeOnlyFunctions.length > 0 ? (
                <SelectGroup>
                  <SelectLabel>{t('builder.code_only_functions')}</SelectLabel>
                  {codeOnlyFunctions.map((option) => (
                    <FunctionSelectItem
                      key={option.name}
                      option={option}
                      label={option.name}
                      disabled
                    />
                  ))}
                </SelectGroup>
              ) : null}
            </SelectContent>
          </Select>
        </BuilderField>

        <BuilderField label={t('builder.aggregation')}>
          <Select
            value={query.aggregation}
            onValueChange={(value) =>
              updateQuery({
                ...query,
                aggregation: value as BuilderAggregation,
              })
            }
          >
            <SelectTrigger
              aria-label={t('builder.aggregation_select_aria')}
              className="h-11 rounded-md border-bd-1 bg-bg-2 text-base sm:h-9 sm:text-sm"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {BUILDER_AGGREGATIONS.map((aggregation) => (
                <SelectItem key={aggregation} value={aggregation}>
                  {t(`builder.aggregations.${aggregation}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </BuilderField>

        <BuilderField label={t('builder.range')}>
          <Select
            value={query.range}
            disabled={selectedFunction?.input !== 'range'}
            onValueChange={(range) => updateQuery({ ...query, range })}
          >
            <SelectTrigger
              aria-label={t('builder.range_select_aria')}
              className="h-11 rounded-md border-bd-1 bg-bg-2 text-base sm:h-9 sm:text-sm"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {BUILDER_RANGES.map((range) => (
                <SelectItem key={range} value={range}>
                  {range}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </BuilderField>
      </div>

      <div className="border-t border-bd-0 px-3 py-3">
        <div className="mb-2 flex min-h-8 items-center justify-between gap-3">
          <span className="font-sans text-xs font-semibold text-tx-1">
            {t('builder.label_matchers')}
          </span>
          <button
            type="button"
            onClick={() =>
              updateQuery({
                ...query,
                matchers: [
                  ...query.matchers,
                  newBuilderMatcher(nextMatcherIndex(query.matchers)),
                ],
              })
            }
            disabled={!query.metric || availableLabels.length === 0}
            className="inline-flex h-11 items-center gap-1.5 rounded-md px-2.5 font-sans text-xs font-semibold text-blue-soft hover:bg-bg-2 hover:text-blue focus-visible:bg-bg-2 focus-visible:text-blue disabled:cursor-not-allowed disabled:text-tx-4 sm:h-8"
          >
            <Plus className="h-3.5 w-3.5" aria-hidden="true" />
            {t('builder.add_matcher')}
          </button>
        </div>

        {query.matchers.length === 0 ? (
          <div className="rounded-md border border-dashed border-bd-0 px-3 py-2 font-sans text-xs text-tx-3">
            {query.metric && availableLabels.length === 0
              ? t('builder.no_labels')
              : t('builder.no_matchers')}
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            {query.matchers.map((matcher, index) => (
              <div
                key={matcher.id}
                className="grid grid-cols-[minmax(0,1fr)_88px_44px] gap-2 sm:grid-cols-[minmax(0,1fr)_88px_minmax(0,1fr)_44px]"
              >
                <Select
                  {...(matcher.name ? { value: matcher.name } : {})}
                  onValueChange={(name) =>
                    updateMatcher(index, { ...matcher, name })
                  }
                >
                  <SelectTrigger
                    aria-label={t('builder.matcher_name_aria', {
                      index: index + 1,
                    })}
                    className="h-11 rounded-md border-bd-1 bg-bg-2 text-base sm:h-9 sm:text-sm"
                  >
                    <SelectValue
                      placeholder={t('builder.matcher_name_placeholder')}
                    />
                  </SelectTrigger>
                  <SelectContent>
                    {availableLabels.map((label) => (
                      <SelectItem key={label} value={label}>
                        {label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>

                <Select
                  value={matcher.operator}
                  onValueChange={(operator) =>
                    updateMatcher(index, {
                      ...matcher,
                      operator: operator as BuilderMatcherOperator,
                    })
                  }
                >
                  <SelectTrigger
                    aria-label={t('builder.matcher_op_aria', {
                      index: index + 1,
                    })}
                    className="h-11 rounded-md border-bd-1 bg-bg-2 text-base sm:h-9 sm:text-sm"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {(['=', '!=', '=~', '!~'] as const).map((operator) => (
                      <SelectItem key={operator} value={operator}>
                        {operator}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>

                <input
                  value={matcher.value}
                  onChange={(event) =>
                    updateMatcher(index, {
                      ...matcher,
                      value: event.target.value,
                    })
                  }
                  aria-label={t('builder.matcher_value_aria', {
                    index: index + 1,
                  })}
                  placeholder={t('builder.matcher_value_placeholder')}
                  className="col-span-3 row-start-2 h-11 min-w-0 rounded-md border border-bd-1 bg-bg-2 px-3 font-sans text-base text-tx-0 placeholder:text-tx-3 focus:outline-none sm:col-span-1 sm:row-auto sm:h-9 sm:text-sm"
                />
                <button
                  type="button"
                  onClick={() =>
                    updateQuery({
                      ...query,
                      matchers: query.matchers.filter(
                        (_item, itemIndex) => itemIndex !== index,
                      ),
                    })
                  }
                  aria-label={t('builder.remove_matcher_aria', {
                    index: index + 1,
                  })}
                  className="col-start-3 row-start-1 grid h-11 w-11 place-items-center rounded-md text-tx-2 hover:bg-bg-2 hover:text-red-soft focus-visible:bg-bg-2 focus-visible:text-red-soft sm:col-auto sm:row-auto sm:h-9 sm:w-9"
                >
                  <X className="h-3.5 w-3.5" aria-hidden="true" />
                </button>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="flex min-h-11 items-center gap-3 border-t border-bd-0 bg-bg-2/50 px-3 py-2">
        <span className="type-micro shrink-0 font-sans font-semibold uppercase tracking-wide text-tx-3">
          {t('builder.editor_label')}
        </span>
        <code
          data-testid="promql-builder-preview"
          className="min-w-0 flex-1 truncate font-mono text-xs text-tx-1"
        >
          {composedExpression || t('builder.preview_empty')}
        </code>
      </div>
    </div>
  );
}

function BuilderField({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="min-w-0">
      <span className="type-micro mb-1.5 block font-sans font-semibold uppercase tracking-wide text-tx-3">
        {label}
      </span>
      {children}
    </div>
  );
}

function FunctionSelectItem({
  option,
  label,
  disabled = false,
}: {
  option: BuilderFunctionOption;
  label: string;
  disabled?: boolean;
}) {
  return (
    <SelectItem
      value={option.name}
      disabled={disabled}
      textValue={label}
      title={option.documentation || option.detail}
      className="h-auto min-h-9 py-1.5"
    >
      <span className="flex min-w-0 flex-col">
        <span className="font-mono text-xs font-semibold">{label}</span>
        {option.detail ? (
          <span className="type-micro truncate font-mono text-tx-3">
            {option.detail}
          </span>
        ) : null}
      </span>
    </SelectItem>
  );
}

function builderFunctionLabel(
  name: string,
  t: TFunction<'metrics'>,
): string {
  switch (name) {
    case 'none':
    case 'rate':
    case 'irate':
    case 'increase':
      return t(`builder.functions.${name}`);
    default:
      return name;
  }
}

function nextMatcherIndex(matchers: BuilderMatcher[]): number {
  return (
    matchers.reduce((highest, matcher) => {
      const parsed = Number.parseInt(matcher.id.replace(/^matcher-/, ''), 10);
      return Number.isFinite(parsed) ? Math.max(highest, parsed) : highest;
    }, -1) + 1
  );
}
