import { Plus, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { ChromeButton, IconButton, uiLabelClass } from '@/shell/chrome';
import { CopyField } from '@/shell/CopyField';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';

import {
  DEFAULT_BUCKETS,
  METRIC_INTERVALS,
  METRIC_TYPES,
  metricQuery,
  type CountingKind,
  type CountingParts,
  type MetricSettings,
  type OverAction,
  type QuotaSettings,
  type StepCondition,
  type ThrottleSettings,
} from './model';
import type { PipelineResources } from '../editor/resources';
import { Segmented } from '../editor/Segmented';
import { StreamField } from '../editor/StreamField';
import { isWritableStream } from '../PipelineGraph/streamName';
import { FILTER_TESTS, filterTestTakesValue, type FilterTest } from '../processors/catalog';
import { FieldList } from '../processors/ProcessorFields';

const FIELD_INPUT = 'font-mono text-xs';
const GRID = 'grid gap-x-[16px] gap-y-[12px] sm:grid-cols-2 xl:grid-cols-3';

/**
 * The form of a step that keeps count: which events it applies to, then what
 * it makes of them or how much of them it lets through.
 */
export function CountingFields({
  kind,
  step,
  source,
  routes,
  resources,
  onChange,
}: {
  kind: CountingKind;
  step: CountingParts;
  /** The stream the pipeline reads: what goes past a limit cannot go back into it. */
  source: string;
  /** Whether what goes past a limit may go to another stream: not where events stay in their stream. */
  routes: boolean;
  resources: PipelineResources | undefined;
  onChange: (patch: CountingParts) => void;
}) {
  return (
    <div className="grid gap-[14px]">
      <ConditionRows conditions={step.conditions ?? []} onChange={(conditions) => onChange({ conditions })} />
      {kind === 'generate_metric' && step.metric && (
        <MetricFields metric={step.metric} resources={resources} onChange={(metric) => onChange({ metric })} />
      )}
      {kind === 'quota' && step.quota && (
        <QuotaFields
          quota={step.quota}
          source={source}
          routes={routes}
          resources={resources}
          onChange={(quota) => onChange({ quota })}
        />
      )}
      {kind === 'throttle' && step.throttle && (
        <ThrottleFields
          throttle={step.throttle}
          source={source}
          routes={routes}
          resources={resources}
          onChange={(throttle) => onChange({ throttle })}
        />
      )}
    </div>
  );
}

/** Field tests that must all hold for the step to apply; none: every event. */
function ConditionRows({
  conditions,
  onChange,
}: {
  conditions: readonly StepCondition[];
  onChange: (conditions: StepCondition[]) => void;
}) {
  const { t } = useTranslation('pipelines');
  const set = (index: number, patch: Partial<StepCondition>) =>
    onChange(conditions.map((item, at) => (at === index ? { ...item, ...patch } : item)));

  return (
    <div className="grid gap-[6px]" role="group" aria-label={t('counting.conditions')}>
      <span className={uiLabelClass}>{t('counting.conditions')}</span>
      {conditions.length === 0 && (
        <span className="font-sans text-xs leading-relaxed text-tx-3">{t('counting.conditions_none')}</span>
      )}
      {conditions.map((condition, index) => (
        <div key={index} data-counting-condition className="flex min-w-0 items-center gap-[8px]">
          <FormInput
            value={condition.field}
            onChange={(event) => set(index, { field: event.target.value })}
            placeholder="level"
            aria-label={t('processors.field')}
            className={`w-[180px] ${FIELD_INPUT}`}
            autoComplete="off"
            spellCheck={false}
          />
          <FormSelect
            value={condition.test}
            onChange={(test) => set(index, { test: test as FilterTest })}
            options={FILTER_TESTS.map((test) => ({ value: test, label: t(`processors.filter_tests.${test}`) }))}
            ariaLabel={t('processors.filter_test')}
            className="w-[150px]"
          />
          {filterTestTakesValue(condition.test) && (
            <FormInput
              value={condition.value}
              onChange={(event) => set(index, { value: event.target.value })}
              placeholder={condition.test === 'matches' ? '^web-' : 'error'}
              aria-label={condition.test === 'matches' ? t('processors.pattern') : t('processors.value')}
              className={`min-w-0 flex-1 ${FIELD_INPUT}`}
              autoComplete="off"
              spellCheck={false}
            />
          )}
          <IconButton
            aria-label={t('counting.remove_condition')}
            onClick={() => onChange(conditions.filter((_, at) => at !== index))}
          >
            <X aria-hidden className="h-3.5 w-3.5" />
          </IconButton>
        </div>
      ))}
      <ChromeButton
        size="sm"
        variant="ghost"
        className="justify-self-start text-tx-2"
        onClick={() => onChange([...conditions, { field: '', test: 'equals', value: '' }])}
      >
        <Plus aria-hidden className="h-3.5 w-3.5" />
        {t('counting.add_condition')}
      </ChromeButton>
    </div>
  );
}

function MetricFields({
  metric,
  resources,
  onChange,
}: {
  metric: MetricSettings;
  resources: PipelineResources | undefined;
  onChange: (metric: MetricSettings) => void;
}) {
  const { t } = useTranslation('pipelines');
  const set = (patch: Partial<MetricSettings>) => onChange({ ...metric, ...patch });

  return (
    <div className="grid gap-[12px]">
      <div className={GRID}>
        <FormField label={t('counting.metric_name')} hint={t('counting.metric_name_hint')}>
          <FormInput
            value={metric.name}
            onChange={(event) => set({ name: event.target.value })}
            placeholder="app_errors_total"
            className={FIELD_INPUT}
            autoComplete="off"
            spellCheck={false}
          />
        </FormField>
        <div className="flex min-w-0 flex-col gap-1.5">
          <span className={uiLabelClass}>{t('counting.metric_type')}</span>
          <Segmented
            label={t('counting.metric_type')}
            value={metric.type}
            onChange={(type) => set({ type, buckets: type === 'histogram' && metric.buckets.length === 0 ? DEFAULT_BUCKETS : metric.buckets })}
            options={METRIC_TYPES.map((type) => ({ value: type, label: t(`counting.metric_types.${type}`) }))}
          />
        </div>
        <FormField
          label={metric.type === 'count' ? t('counting.metric_sum_field') : t('counting.metric_field')}
          hint={metric.type === 'count' ? t('counting.metric_sum_field_hint') : t('counting.metric_field_hint')}
        >
          <FormInput
            value={metric.field}
            onChange={(event) => set({ field: event.target.value })}
            placeholder={metric.type === 'count' ? t('counting.metric_sum_placeholder') : 'duration_ms'}
            className={FIELD_INPUT}
            autoComplete="off"
            spellCheck={false}
          />
        </FormField>
        <FieldList
          label={t('counting.group_by')}
          hint={t('counting.group_by_hint')}
          placeholder="service, status"
          fields={metric.groupBy}
          onChange={(groupBy) => set({ groupBy })}
        />
        {resources && (
          <StreamField
            label={t('counting.metric_stream')}
            hint={t('counting.metric_stream_hint')}
            value={metric.stream}
            signalType="metrics"
            resources={resources}
            creatable
            offers={isWritableStream}
            onChange={(stream) => set({ stream })}
          />
        )}
        <FormField label={t('counting.interval')} hint={t('counting.interval_hint')}>
          <FormSelect
            value={String(metric.intervalSecs)}
            onChange={(value) => set({ intervalSecs: Number(value) })}
            options={METRIC_INTERVALS.map((secs) => ({ value: String(secs), label: t(`counting.intervals.${secs}`) }))}
          />
        </FormField>
      </div>
      {metric.type === 'histogram' && (
        <FieldList
          label={t('counting.buckets')}
          hint={t('counting.buckets_hint')}
          placeholder={DEFAULT_BUCKETS.join(', ')}
          fields={metric.buckets}
          onChange={(buckets) => set({ buckets })}
        />
      )}
      <FormField label={t('counting.query')} hint={t('counting.query_hint')}>
        <CopyField value={metricQuery(metric)} copyLabel={t('counting.copy_query')} />
      </FormField>
    </div>
  );
}

/** What goes past a limit: dropped, let on (quotas), or sent to another stream as it is. */
function OverLimit({
  action,
  target,
  actions,
  source,
  resources,
  onChange,
}: {
  action: OverAction;
  target: string;
  actions: readonly OverAction[];
  source: string;
  resources: PipelineResources | undefined;
  onChange: (patch: { action?: OverAction; target?: string }) => void;
}) {
  const { t } = useTranslation('pipelines');

  return (
    <div className={GRID}>
      <div className="flex min-w-0 flex-col gap-1.5 sm:col-span-2">
        <span className={uiLabelClass}>{t('counting.over_limit')}</span>
        <Segmented
          label={t('counting.over_limit')}
          value={action}
          onChange={(next) => onChange({ action: next })}
          options={actions.map((item) => ({ value: item, label: t(`counting.over_actions.${item}`) }))}
        />
      </div>
      {action === 'route' && resources && (
        <StreamField
          label={t('counting.over_target')}
          hint={t('counting.over_target_hint')}
          value={target}
          signalType="logs"
          resources={resources}
          creatable
          offers={(stream) => isWritableStream(stream) && stream !== source}
          onChange={(next) => onChange({ target: next })}
        />
      )}
    </div>
  );
}

function QuotaFields({
  quota,
  source,
  routes,
  resources,
  onChange,
}: {
  quota: QuotaSettings;
  source: string;
  routes: boolean;
  resources: PipelineResources | undefined;
  onChange: (quota: QuotaSettings) => void;
}) {
  const { t } = useTranslation('pipelines');
  const set = (patch: Partial<QuotaSettings>) => onChange({ ...quota, ...patch });
  const fields = quota.partitionBy.map((field) => field.trim()).filter(Boolean);
  const unit = t(`counting.units.${quota.unit}`);

  return (
    <div className="grid gap-[12px]">
      <div className={GRID}>
        <div className="flex min-w-0 flex-col gap-1.5">
          <span className={uiLabelClass}>{t('counting.quota_unit')}</span>
          <Segmented
            label={t('counting.quota_unit')}
            value={quota.unit}
            onChange={(next) => set({ unit: next })}
            options={(['events', 'bytes'] as const).map((item) => ({ value: item, label: t(`counting.quota_units.${item}`) }))}
          />
        </div>
        <FormField label={t('counting.quota_limit', { unit })} hint={t('counting.quota_limit_hint')}>
          <FormInput
            value={quota.limit}
            onChange={(event) => set({ limit: event.target.value })}
            inputMode="decimal"
            placeholder={quota.unit === 'bytes' ? '10240' : '1000000'}
            className={FIELD_INPUT}
            autoComplete="off"
          />
        </FormField>
        <FieldList
          label={t('counting.partition_by')}
          hint={t('counting.partition_by_hint')}
          placeholder="service"
          fields={quota.partitionBy}
          onChange={(partitionBy) => set({ partitionBy })}
        />
      </div>
      {fields.length > 0 && <Overrides quota={quota} fields={fields} unit={unit} onChange={set} />}
      <OverLimit
        action={quota.action}
        target={quota.target}
        actions={routes ? ['drop', 'pass', 'route'] : ['drop', 'pass']}
        source={source}
        resources={resources}
        onChange={set}
      />
    </div>
  );
}

/** Buckets with a limit of their own, each named by a value of every field the quota is split by. */
function Overrides({
  quota,
  fields,
  unit,
  onChange,
}: {
  quota: QuotaSettings;
  fields: readonly string[];
  unit: string;
  onChange: (patch: Partial<QuotaSettings>) => void;
}) {
  const { t } = useTranslation('pipelines');
  const set = (index: number, patch: Partial<QuotaSettings['overrides'][number]>) =>
    onChange({ overrides: quota.overrides.map((item, at) => (at === index ? { ...item, ...patch } : item)) });

  return (
    <div className="grid gap-[6px]" role="group" aria-label={t('counting.overrides')}>
      <span className={uiLabelClass}>{t('counting.overrides')}</span>
      {quota.overrides.map((override, index) => (
        <div key={index} data-quota-override className="flex min-w-0 flex-wrap items-center gap-[8px]">
          {fields.map((field) => (
            <FormInput
              key={field}
              value={override.values[field] ?? ''}
              onChange={(event) => set(index, { values: { ...override.values, [field]: event.target.value } })}
              placeholder={field}
              aria-label={field}
              className={`w-[160px] ${FIELD_INPUT}`}
              autoComplete="off"
              spellCheck={false}
            />
          ))}
          <FormInput
            value={override.limit}
            onChange={(event) => set(index, { limit: event.target.value })}
            placeholder={t('counting.override_limit', { unit })}
            aria-label={t('counting.override_limit', { unit })}
            inputMode="decimal"
            className={`w-[140px] ${FIELD_INPUT}`}
            autoComplete="off"
          />
          <IconButton
            aria-label={t('counting.remove_override')}
            onClick={() => onChange({ overrides: quota.overrides.filter((_, at) => at !== index) })}
          >
            <X aria-hidden className="h-3.5 w-3.5" />
          </IconButton>
        </div>
      ))}
      <ChromeButton
        size="sm"
        variant="ghost"
        className="justify-self-start text-tx-2"
        onClick={() => onChange({ overrides: [...quota.overrides, { values: {}, limit: '' }] })}
      >
        <Plus aria-hidden className="h-3.5 w-3.5" />
        {t('counting.add_override')}
      </ChromeButton>
    </div>
  );
}

function ThrottleFields({
  throttle,
  source,
  routes,
  resources,
  onChange,
}: {
  throttle: ThrottleSettings;
  source: string;
  routes: boolean;
  resources: PipelineResources | undefined;
  onChange: (throttle: ThrottleSettings) => void;
}) {
  const { t } = useTranslation('pipelines');
  const set = (patch: Partial<ThrottleSettings>) => onChange({ ...throttle, ...patch });

  return (
    <div className="grid gap-[12px]">
      <div className={GRID}>
        <FormField label={t('counting.throttle_limit')} hint={t('counting.throttle_limit_hint')}>
          <FormInput
            value={throttle.limit}
            onChange={(event) => set({ limit: event.target.value })}
            inputMode="numeric"
            placeholder="1000"
            className={FIELD_INPUT}
            autoComplete="off"
          />
        </FormField>
        <FormField label={t('counting.throttle_window')}>
          <FormInput
            value={throttle.windowSecs}
            onChange={(event) => set({ windowSecs: event.target.value })}
            inputMode="numeric"
            placeholder="60"
            className={FIELD_INPUT}
            autoComplete="off"
          />
        </FormField>
        <FieldList
          label={t('counting.throttle_group_by')}
          hint={t('counting.throttle_group_by_hint')}
          placeholder="host"
          fields={throttle.groupBy}
          onChange={(groupBy) => set({ groupBy })}
        />
      </div>
      <OverLimit
        action={throttle.action}
        target={throttle.target}
        actions={routes ? ['drop', 'route'] : ['drop']}
        source={source}
        resources={resources}
        onChange={(patch) =>
          set({
            ...(patch.action && patch.action !== 'pass' && { action: patch.action }),
            ...(patch.target !== undefined && { target: patch.target }),
          })
        }
      />
    </div>
  );
}
