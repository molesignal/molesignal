import { Plus, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { AnalyticsStepKind } from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { defaultField, MAX_STEPS, MIN_STEPS, STEP_FIELDS, type StepDraft, type StepOp } from './steps';
import { useSuggestions } from './useAnalytics';
import { BREAKDOWNS, WINDOWS } from './useFunnelAddress';
import { ValueCombobox } from './ValueCombobox';
import type { OverviewRange, OverviewScope } from '../overview/model';
import { CONTROL } from '../overview/ScopeRow';

const NONE = '__none__';

/** A key of the catalog for a field: `view.url_path_group` is `view_url_path_group`. */
const fieldKey = (field: string) => field.replaceAll('.', '_');

function StepRow({
  index,
  step,
  browser,
  scope,
  range,
  removable,
  onChange,
  onRemove,
}: {
  index: number;
  step: StepDraft;
  browser: boolean;
  scope: OverviewScope;
  range: OverviewRange;
  removable: boolean;
  onChange: (step: StepDraft) => void;
  onRemove: () => void;
}) {
  const { t } = useTranslation('rum');
  const suggestions = useSuggestions(step.kind, step.field, scope, range);
  const values = (suggestions.data?.values ?? []).map((entry) => entry.value).filter((value) => value !== '');

  const kinds: AnalyticsStepKind[] = ['views', 'actions'];

  return (
    <li data-rum-funnel-step-row={index} className="flex flex-wrap items-center gap-2">
      <span aria-hidden className="grid h-6 w-6 shrink-0 place-items-center rounded-full bg-bg-3 font-sans text-xs font-strong tabular-nums text-tx-1">
        {index + 1}
      </span>
      <FittedSelect
        aria-label={t('analytics.funnel.step_kind', { number: index + 1 })}
        data-rum-step-kind=""
        value={step.kind}
        onValueChange={(kind) => onChange({ kind: kind as AnalyticsStepKind, field: defaultField(kind as AnalyticsStepKind, browser), op: '=', value: '' })}
        options={kinds.map((kind) => ({ value: kind, label: t(`analytics.kinds.${kind}${kind === 'views' && !browser ? '_app' : ''}`) }))}
        maxWidth={200}
        className={cn(CONTROL, 'font-strong text-tx-1')}
      />
      <FittedSelect
        aria-label={t('analytics.funnel.step_field', { number: index + 1 })}
        data-rum-step-field=""
        value={step.field}
        onValueChange={(field) => onChange({ ...step, field, value: '' })}
        options={STEP_FIELDS[step.kind].map((field) => ({ value: field, label: t(`analytics.fields.${fieldKey(field)}`) }))}
        maxWidth={200}
        className={cn(CONTROL, 'text-tx-1')}
      />
      <FittedSelect
        aria-label={t('analytics.funnel.step_op', { number: index + 1 })}
        data-rum-step-op=""
        value={step.op}
        onValueChange={(op) => onChange({ ...step, op: op as StepOp })}
        options={[
          { value: '=', label: t('analytics.ops.eq') },
          { value: 'contains', label: t('analytics.ops.contains') },
        ]}
        maxWidth={140}
        className={cn(CONTROL, 'text-tx-1')}
      />
      <ValueCombobox
        label={t('analytics.funnel.step_value', { number: index + 1 })}
        value={step.value}
        onChange={(value) => onChange({ ...step, value })}
        suggestions={values}
        placeholder={t('analytics.funnel.step_placeholder')}
        className="w-[260px] max-w-full"
      />
      {removable && (
        <button
          type="button"
          aria-label={t('analytics.funnel.remove_step', { number: index + 1 })}
          data-rum-step-remove=""
          onClick={onRemove}
          className="grid h-6 w-6 place-items-center rounded-md border-0 bg-transparent p-0 text-tx-3 outline-none hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
        >
          <X aria-hidden className="h-3.5 w-3.5" />
        </button>
      )}
    </li>
  );
}

/**
 * The steps of a funnel, one row each: what the step looks at (a page or an action), in which
 * field, and for what. Under them how long a session has to get through it and what the sessions
 * are counted by.
 */
export function FunnelBuilder({
  steps,
  onSteps,
  windowSecs,
  onWindow,
  breakdown,
  onBreakdown,
  browser,
  scope,
  range,
}: {
  steps: readonly StepDraft[];
  onSteps: (steps: readonly StepDraft[]) => void;
  windowSecs: number | null;
  onWindow: (seconds: number | null) => void;
  breakdown: string;
  onBreakdown: (field: string) => void;
  browser: boolean;
  scope: OverviewScope;
  range: OverviewRange;
}) {
  const { t } = useTranslation('rum');

  const replace = (index: number, step: StepDraft) => onSteps(steps.map((candidate, at) => (at === index ? step : candidate)));
  const last = steps[steps.length - 1];

  return (
    <div data-rum-funnel-builder="" className="space-y-3 px-3 py-3">
      <ol className="m-0 flex list-none flex-col gap-2 p-0">
        {steps.map((step, index) => (
          <StepRow
            // A step has no identity but its place.
            key={index}
            index={index}
            step={step}
            browser={browser}
            scope={scope}
            range={range}
            removable={steps.length > MIN_STEPS}
            onChange={(next) => replace(index, next)}
            onRemove={() => onSteps(steps.filter((_, at) => at !== index))}
          />
        ))}
      </ol>
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          data-rum-funnel-add=""
          disabled={steps.length >= MAX_STEPS}
          onClick={() => onSteps([...steps, { kind: last?.kind ?? 'views', field: last?.field ?? defaultField('views', browser), op: '=', value: '' }])}
          className="inline-flex h-8 items-center gap-1.5 rounded-md border-0 bg-[var(--control-surface)] px-2.5 font-sans text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3 disabled:pointer-events-none disabled:text-[var(--tx-disabled)]"
        >
          <Plus aria-hidden className="h-3.5 w-3.5 text-tx-3" />
          {t('analytics.funnel.add_step')}
        </button>
        <FittedSelect
          aria-label={t('analytics.funnel.window')}
          data-rum-funnel-window=""
          value={windowSecs ? String(windowSecs) : NONE}
          onValueChange={(value) => onWindow(value === NONE ? null : Number(value))}
          options={[
            { value: NONE, label: t('analytics.funnel.window_none') },
            ...WINDOWS.map((seconds) => {
              const { unit, count } = windowUnit(seconds);

              return { value: String(seconds), label: t('analytics.funnel.window_within', { time: t(`analytics.funnel.${unit}`, { count }) }) };
            }),
          ]}
          maxWidth={220}
          className={cn(CONTROL, 'text-tx-1')}
        />
        <FittedSelect
          aria-label={t('analytics.funnel.breakdown')}
          data-rum-funnel-breakdown=""
          value={breakdown || NONE}
          onValueChange={(value) => onBreakdown(value === NONE ? '' : value)}
          options={[
            { value: NONE, label: t('analytics.funnel.breakdown_none') },
            ...BREAKDOWNS.map((field) => ({ value: field, label: t('analytics.funnel.breakdown_by', { field: t(`analytics.breakdowns.${fieldKey(field)}`) }) })),
          ]}
          maxWidth={240}
          className={cn(CONTROL, 'text-tx-1')}
        />
      </div>
    </div>
  );
}

/** A time limit as a whole number of minutes, hours or days. */
function windowUnit(seconds: number): { unit: 'minutes' | 'hours' | 'days'; count: number } {
  if (seconds < 3_600) return { unit: 'minutes', count: seconds / 60 };
  if (seconds < 86_400) return { unit: 'hours', count: seconds / 3_600 };

  return { unit: 'days', count: seconds / 86_400 };
}
