import { cn } from '@/shell/lib/cn';

import { useDashboardText } from '../../i18n';

type VariableQueryMode = 'label_values' | 'classic' | 'sql';

const QUERY_MODES: ReadonlyArray<readonly [VariableQueryMode, string]> = [
  ['label_values', 'Label values'],
  ['classic', 'Classic query'],
  ['sql', 'SQL'],
];

const STREAM_TYPES = ['logs', 'metrics', 'traces'] as const;

const CONTROL_CLASS =
  'min-h-11 min-w-0 rounded-md border border-bd-1 bg-bg-1 px-2 font-sans text-base text-tx-1 outline-none placeholder:text-tx-3 focus-visible:bg-bg-2 sm:min-h-8 sm:text-xs';

export function VariableQueryEditor({
  value,
  onChange,
}: {
  value: Record<string, unknown>;
  onChange: (value: Record<string, unknown>) => void;
}) {
  const tr = useDashboardText();
  const expression = stringValue(value.expression ?? value.query);
  const parsedLabelValues = parseLabelValuesExpression(expression);
  const mode = resolveQueryMode(value, parsedLabelValues);
  const metric = stringValue(value.metric) || parsedLabelValues?.metric || '';
  const label = stringValue(value.label) || parsedLabelValues?.label || '';

  const updateLabelValues = (nextMetric: string, nextLabel: string) =>
    onChange({
      ...value,
      kind: 'query',
      queryType: 'label_values',
      metric: nextMetric,
      label: nextLabel,
      expression: buildLabelValuesExpression(nextMetric, nextLabel),
    });

  const selectMode = (nextMode: VariableQueryMode) => {
    if (nextMode === 'label_values') {
      updateLabelValues(metric, label);
      return;
    }
    onChange({
      ...value,
      queryType: nextMode,
      kind: nextMode === 'sql' ? 'sql' : 'query',
      expression:
        nextMode === 'classic' && mode !== 'sql' ? expression : '',
    });
  };

  return (
    <div className="grid gap-2">
      <div className="grid gap-2 sm:grid-cols-3">
        <VariableQueryField label="Query type">
          <select
            value={mode}
            aria-label={tr('Query type')}
            onChange={(event) =>
              selectMode(event.target.value as VariableQueryMode)
            }
            className={CONTROL_CLASS}
          >
            {QUERY_MODES.map(([optionValue, optionLabel]) => (
              <option key={optionValue} value={optionValue}>
                {tr(optionLabel)}
              </option>
            ))}
          </select>
        </VariableQueryField>

        {mode === 'label_values' ? (
          <>
            <VariableQueryField label="Metric">
              <input
                value={metric}
                aria-label={tr('Metric')}
                placeholder="http_requests_total"
                onChange={(event) =>
                  updateLabelValues(event.target.value, label)
                }
                className={cn(CONTROL_CLASS, 'font-mono')}
              />
            </VariableQueryField>
            <VariableQueryField label="Label">
              <input
                value={label}
                aria-label={tr('Label')}
                placeholder="service"
                onChange={(event) =>
                  updateLabelValues(metric, event.target.value)
                }
                className={cn(CONTROL_CLASS, 'font-mono')}
              />
            </VariableQueryField>
          </>
        ) : mode === 'sql' ? (
          <>
            <VariableQueryField label="Stream name">
              <input
                value={stringValue(value.streamName ?? value.stream)}
                aria-label={tr('Stream name')}
                placeholder="stream"
                onChange={(event) =>
                  onChange({ ...value, streamName: event.target.value })
                }
                className={cn(CONTROL_CLASS, 'font-mono')}
              />
            </VariableQueryField>
            <VariableQueryField label="Stream type">
              <select
                value={stringValue(value.streamType) || 'logs'}
                aria-label={tr('Stream type')}
                onChange={(event) =>
                  onChange({ ...value, streamType: event.target.value })
                }
                className={CONTROL_CLASS}
              >
                {STREAM_TYPES.map((streamType) => (
                  <option key={streamType} value={streamType}>
                    {tr(streamType)}
                  </option>
                ))}
              </select>
            </VariableQueryField>
          </>
        ) : null}
      </div>

      {mode !== 'label_values' && (
        <VariableQueryField label={mode === 'sql' ? 'SQL query' : 'Query'}>
          <textarea
            value={expression}
            rows={mode === 'sql' ? 4 : 2}
            spellCheck={false}
            aria-label={tr(mode === 'sql' ? 'SQL query' : 'Query')}
            placeholder={
              mode === 'sql'
                ? 'SELECT DISTINCT service FROM http_requests_total'
                : 'label_values(http_requests_total, service)'
            }
            onChange={(event) =>
              onChange({
                ...value,
                kind: mode === 'sql' ? 'sql' : 'query',
                queryType: mode,
                expression: event.target.value,
              })
            }
            className={cn(
              CONTROL_CLASS,
              'resize-y py-2 font-mono leading-5',
            )}
          />
        </VariableQueryField>
      )}
    </div>
  );
}

function VariableQueryField({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  const tr = useDashboardText();
  return (
    <label className="grid min-w-0 gap-1 font-sans text-xs font-medium text-tx-3">
      {tr(label)}
      {children}
    </label>
  );
}

function resolveQueryMode(
  value: Record<string, unknown>,
  parsedLabelValues: { metric: string; label: string } | null,
): VariableQueryMode {
  const explicit = stringValue(value.queryType);
  if (
    explicit === 'label_values' ||
    explicit === 'classic' ||
    explicit === 'sql'
  ) {
    return explicit;
  }
  if (value.kind === 'sql') return 'sql';
  const expression = stringValue(value.expression ?? value.query).trim();
  return !expression || parsedLabelValues ? 'label_values' : 'classic';
}

function parseLabelValuesExpression(
  expression: string,
): { metric: string; label: string } | null {
  const match = expression.match(
    /^\s*label_values\(\s*([A-Za-z_][A-Za-z0-9_:.-]*)\s*,\s*([A-Za-z_][A-Za-z0-9_:.-]*)\s*\)\s*$/,
  );
  return match?.[1] && match[2]
    ? { metric: match[1], label: match[2] }
    : null;
}

function buildLabelValuesExpression(metric: string, label: string): string {
  const normalizedMetric = metric.trim();
  const normalizedLabel = label.trim();
  return normalizedMetric && normalizedLabel
    ? `label_values(${normalizedMetric}, ${normalizedLabel})`
    : '';
}

function stringValue(value: unknown): string {
  return typeof value === 'string' ? value : '';
}
