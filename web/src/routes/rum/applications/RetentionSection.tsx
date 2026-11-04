import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { RETENTION_FIELDS } from '@/api/rum';
import type { RetentionCondition, RetentionField, RetentionFilter, RetentionOp, RumApplicationSettings } from '@/api/rum';
import { FieldArray, FormField, FormInput, FormSection, FormSelect } from '@/shell/FormDrawer';

import { MAX_SESSION_SAMPLE_RATE } from './model';
import {
  MAX_FILTER_NAME,
  MAX_RETENTION_CONDITIONS,
  MAX_RETENTION_FILTERS,
  newCondition,
  newFilter,
  parseValues,
  sameValues,
} from './retention';
import { SettingSwitch } from './SettingSwitch';

/** The key a field is written under in the translations: the dots of its name are underscores. */
const keyOf = (field: RetentionField) => field.replaceAll('.', '_');

/**
 * What an application keeps of its sessions: the filters that give a share to the sessions they
 * describe, the share of all the others, and whether the errors of sessions that are not kept are
 * kept all the same.
 */
export function RetentionSection({
  settings,
  onChange,
  disabled,
}: {
  settings: RumApplicationSettings;
  onChange: (settings: RumApplicationSettings) => void;
  disabled: boolean;
}) {
  const { t } = useTranslation('rum');
  const filters = settings.retentionFilters;
  const rest = settings.sessionSampleRate;

  return (
    <FormSection title={t('applications.drawer.sampling_title')} description={t('applications.drawer.sampling_description')}>
      {/* A disabled fieldset disables every control in it, the add and remove buttons too. */}
      <fieldset disabled={disabled} className="m-0 flex min-w-0 flex-col gap-4 border-0 p-0" data-rum-retention="">
        <FieldArray<RetentionFilter>
          items={filters}
          onChange={(retention_filters) => onChange({ ...settings, retentionFilters: retention_filters })}
          reorderable
          maxItems={MAX_RETENTION_FILTERS}
          newItem={() =>
            newFilter(
              filters.map((filter) => filter.name),
              (number) => t('applications.drawer.retention.new_name', { number }),
            )
          }
          addLabel={t('applications.drawer.retention.add_filter')}
          removeLabel={t('applications.drawer.retention.remove_filter')}
          reorderLabel={{
            up: t('applications.drawer.retention.filter_up'),
            down: t('applications.drawer.retention.filter_down'),
          }}
          emptyHint={t('applications.drawer.retention.no_filters')}
          renderItem={(filter, index, setFilter) => (
            <FilterCard
              filter={filter}
              repeated={filters.some(
                (other, at) => at !== index && other.name.trim().toLowerCase() === filter.name.trim().toLowerCase(),
              )}
              onChange={setFilter}
            />
          )}
        />

        <FormField
          label={t(filters.length === 0 ? 'applications.drawer.sample_rate' : 'applications.drawer.retention.rest_rate')}
          hint={t(filters.length === 0 ? 'applications.drawer.sample_rate_hint' : 'applications.drawer.retention.rest_rate_hint')}
        >
          <PercentInput
            value={rest}
            label={t(filters.length === 0 ? 'applications.drawer.sample_rate' : 'applications.drawer.retention.rest_rate')}
            onChange={(session_sample_rate) => onChange({ ...settings, sessionSampleRate: session_sample_rate })}
          />
        </FormField>

        <SettingSwitch
          label={t('applications.drawer.retention.keep_errors')}
          hint={t('applications.drawer.retention.keep_errors_hint')}
          checked={settings.keepErrors}
          disabled={disabled}
          onChange={(keep_errors) => onChange({ ...settings, keepErrors: keep_errors })}
        />
      </fieldset>
    </FormSection>
  );
}

/** A number of percent that may be empty while it is typed. */
function PercentInput({
  value,
  label,
  onChange,
  className = 'w-28',
}: {
  value: number;
  label: string;
  onChange: (value: number) => void;
  className?: string;
}) {
  const bad = !Number.isInteger(value) || value < 0 || value > MAX_SESSION_SAMPLE_RATE;

  return (
    <FormInput
      type="number"
      inputMode="numeric"
      min={0}
      max={MAX_SESSION_SAMPLE_RATE}
      step={1}
      value={Number.isNaN(value) ? '' : String(value)}
      onChange={(event) => onChange(event.target.value === '' ? Number.NaN : Number(event.target.value))}
      aria-label={label}
      aria-invalid={bad || undefined}
      className={`${className} tabular-nums`}
    />
  );
}

function FilterCard({
  filter,
  repeated,
  onChange,
}: {
  filter: RetentionFilter;
  /** Another filter has this name. */
  repeated: boolean;
  onChange: (filter: RetentionFilter) => void;
}) {
  const { t } = useTranslation('rum');
  const named = filter.name.trim() !== '' && !repeated;

  return (
    <div data-rum-retention-filter="" className="flex min-w-0 flex-col gap-2.5 rounded-md bg-bg-2 p-3">
      <div className="flex items-center gap-2">
        <FormInput
          value={filter.name}
          maxLength={MAX_FILTER_NAME}
          onChange={(event) => onChange({ ...filter, name: event.target.value })}
          aria-label={t('applications.drawer.retention.filter_name')}
          aria-invalid={!named || undefined}
          placeholder={t('applications.drawer.retention.filter_name')}
        />
        <PercentInput
          value={filter.rate}
          label={t('applications.drawer.retention.filter_rate')}
          onChange={(rate) => onChange({ ...filter, rate })}
          className="w-20 shrink-0"
        />
        <span aria-hidden className="shrink-0 font-sans text-xs text-tx-3">
          %
        </span>
      </div>

      <FieldArray<RetentionCondition>
        items={filter.conditions}
        onChange={(conditions) => onChange({ ...filter, conditions })}
        maxItems={MAX_RETENTION_CONDITIONS}
        newItem={newCondition}
        addLabel={t('applications.drawer.retention.add_condition')}
        removeLabel={t('applications.drawer.retention.remove_condition')}
        renderItem={(condition, _index, setCondition) => <ConditionRow condition={condition} onChange={setCondition} />}
      />
    </div>
  );
}

function ConditionRow({
  condition,
  onChange,
}: {
  condition: RetentionCondition;
  onChange: (condition: RetentionCondition) => void;
}) {
  const { t } = useTranslation('rum');

  return (
    <div data-rum-retention-condition="" className="grid min-w-0 grid-cols-[minmax(0,1fr)_6rem] items-center gap-2">
      <FormSelect
        value={condition.field}
        onChange={(field) => onChange({ ...condition, field: field as RetentionField })}
        ariaLabel={t('applications.drawer.retention.condition_field')}
        options={RETENTION_FIELDS.map((field) => ({
          value: field,
          label: t(`applications.drawer.retention.fields.${keyOf(field)}`),
        }))}
      />
      <FormSelect
        value={condition.op}
        onChange={(op) => onChange({ ...condition, op: op as RetentionOp })}
        ariaLabel={t('applications.drawer.retention.condition_op')}
        options={[
          { value: 'is', label: t('applications.drawer.retention.ops.is') },
          { value: 'is_not', label: t('applications.drawer.retention.ops.is_not') },
        ]}
      />
      <div className="col-span-2">
        <ValuesInput
          values={condition.values}
          onChange={(values) => onChange({ ...condition, values })}
          label={t('applications.drawer.retention.condition_values')}
          placeholder={t('applications.drawer.retention.values_placeholder')}
        />
      </div>
    </div>
  );
}

/** The values of a condition, typed as a list between commas. What is typed is kept as it is while it is typed. */
export function ValuesInput({
  values,
  onChange,
  label,
  placeholder,
}: {
  values: readonly string[];
  onChange: (values: string[]) => void;
  label: string;
  placeholder: string;
}) {
  const [draft, setDraft] = React.useState(() => values.join(', '));

  // A list that is not the one typed (a filter moved or removed, a form that was reset) is shown as it is.
  React.useEffect(() => {
    setDraft((current) => (sameValues(parseValues(current), values) ? current : values.join(', ')));
  }, [values]);

  return (
    <FormInput
      value={draft}
      onChange={(event) => {
        setDraft(event.target.value);
        onChange(parseValues(event.target.value));
      }}
      aria-label={label}
      aria-invalid={values.length === 0 || undefined}
      placeholder={placeholder}
    />
  );
}
