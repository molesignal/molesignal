import { ChevronDown, ChevronUp } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { QueryLegendControl } from '@/dashboard-engine/query/editor/QueryLegendControl';
import { resolveQueryLegendMode } from '@/dashboard-engine/query/legend';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

import {
  isValidMetricsStep,
  type MetricsQueryOptions,
  type MetricsQueryType,
  type MetricsResultFormat,
} from './model';

interface QueryOptionsEditorProps {
  value: MetricsQueryOptions;
  onChange: (value: MetricsQueryOptions) => void;
}

export function QueryOptionsEditor({ value, onChange }: QueryOptionsEditorProps) {
  const { t } = useTranslation('metrics');
  const [open, setOpen] = React.useState(false);
  const setOption = React.useCallback(
    <Key extends keyof MetricsQueryOptions>(
      key: Key,
      nextValue: MetricsQueryOptions[Key],
    ) => onChange({ ...value, [key]: nextValue }),
    [onChange, value],
  );
  const stepValid = isValidMetricsStep(value.step);
  const legendMode = resolveQueryLegendMode(value.legend);
  const legendSummary = legendMode === 'custom'
    ? value.legend!
    : t(`explore.query.${legendMode}`);

  return (
    <div className="border-t border-bd-0 bg-bg-2/50">
      <button
        type="button"
        data-testid="metrics-options-toggle"
        aria-expanded={open}
        onClick={() => setOpen((current) => !current)}
        className="flex min-h-10 w-full flex-wrap items-center gap-x-4 gap-y-1 px-3 py-2 text-left font-sans text-xs text-tx-3 hover:bg-bg-2 focus-visible:bg-bg-2"
      >
        {open ? (
          <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
        ) : (
          <ChevronUp className="h-3.5 w-3.5" aria-hidden="true" />
        )}
        <strong className="font-semibold text-tx-1">
          {t('explore.query.options')}
        </strong>
        <OptionSummary label={t('explore.query.legend')} value={legendSummary} />
        <OptionSummary
          label={t('explore.query.format')}
          value={t(`explore.query.${value.format}`)}
        />
        <OptionSummary
          label={t('explore.query.step')}
          value={value.step.trim() || t('explore.query.auto').toLowerCase()}
        />
        <OptionSummary
          label={t('explore.query.type')}
          value={t(`explore.query.${value.type}`)}
        />
        <OptionSummary
          label={t('explore.query.exemplars')}
          value={t(value.exemplars ? 'explore.query.on' : 'explore.query.off')}
        />
      </button>

      {open ? (
        <div
          className="grid grid-cols-1 gap-2 border-t border-bd-0 bg-bg-1 p-3 sm:grid-cols-2 lg:grid-cols-5"
          data-testid="metrics-query-options"
        >
          <OptionField label={t('explore.query.legend')}>
            <div data-testid="metrics-option-legend">
              <QueryLegendControl
                value={value.legend}
                onChange={(legend) => setOption('legend', legend)}
              />
            </div>
          </OptionField>

          <OptionField label={t('explore.query.format')}>
            <OptionSelect
              ariaLabel={t('explore.query.format')}
              value={value.format}
              onValueChange={(next) =>
                setOption('format', next as MetricsResultFormat)
              }
              items={[
                ['time_series', t('explore.query.time_series')],
                ['table', t('explore.query.table')],
              ]}
              testId="metrics-option-format"
            />
          </OptionField>

          <OptionField
            label={t('explore.query.step')}
            hint={stepValid ? undefined : t('explore.query.step_invalid')}
          >
            <input
              type="text"
              value={value.step}
              onChange={(event) => setOption('step', event.target.value)}
              placeholder={t('explore.query.step_placeholder')}
              aria-label={t('explore.query.step')}
              aria-invalid={!stepValid || undefined}
              data-testid="metrics-option-step"
              className="h-11 w-full rounded-md border border-bd-1 bg-bg-2 px-3 font-mono text-base text-tx-0 outline-none placeholder:text-tx-3 hover:border-bd-2 focus:bg-bg-3 sm:h-9 sm:text-sm"
            />
          </OptionField>

          <OptionField label={t('explore.query.type')}>
            <OptionSelect
              ariaLabel={t('explore.query.type')}
              value={value.type}
              onValueChange={(next) =>
                setOption('type', next as MetricsQueryType)
              }
              items={[
                ['range', t('explore.query.range')],
                ['instant', t('explore.query.instant')],
              ]}
              testId="metrics-option-type"
            />
          </OptionField>

          <OptionField label={t('explore.query.exemplars')}>
            <OptionSelect
              ariaLabel={t('explore.query.exemplars')}
              value={value.exemplars ? 'on' : 'off'}
              onValueChange={(next) => setOption('exemplars', next === 'on')}
              items={[
                ['on', t('explore.query.on')],
                ['off', t('explore.query.off')],
              ]}
              testId="metrics-option-exemplars"
            />
          </OptionField>
        </div>
      ) : null}
    </div>
  );
}

function OptionSummary({ label, value }: { label: string; value: string }) {
  return (
    <span className="hidden min-w-0 sm:inline">
      {label}:{' '}
      <span className="inline-block max-w-40 truncate align-bottom text-tx-1">
        {value}
      </span>
    </span>
  );
}

function OptionField({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string | undefined;
  children: React.ReactNode;
}) {
  return (
    <label className="min-w-0 font-sans">
      <span className="mb-1.5 block font-sans text-xs font-medium text-tx-2">
        {label}
      </span>
      {children}
      {hint ? (
        <span className="type-micro mt-1 block text-red-soft">{hint}</span>
      ) : null}
    </label>
  );
}

function OptionSelect({
  ariaLabel,
  value,
  onValueChange,
  items,
  testId,
}: {
  ariaLabel: string;
  value: string;
  onValueChange: (value: string) => void;
  items: ReadonlyArray<readonly [string, string]>;
  testId: string;
}) {
  return (
    <Select value={value} onValueChange={onValueChange}>
      <SelectTrigger
        aria-label={ariaLabel}
        data-testid={testId}
        className="h-11 bg-bg-2 text-base focus:bg-bg-3 sm:h-9 sm:text-sm"
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {items.map(([itemValue, label]) => (
          <SelectItem key={itemValue} value={itemValue}>
            {label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
