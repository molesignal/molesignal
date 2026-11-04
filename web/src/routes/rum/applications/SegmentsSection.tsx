import { useTranslation } from 'react-i18next';

import { SEGMENT_FIELDS, SEGMENT_USER_PREFIX } from '@/api/rum';
import type { RumApplicationSettings, RumSegment, RumSegmentCondition, RetentionOp } from '@/api/rum';
import { FieldArray, FormInput, FormSection, FormSelect } from '@/shell/FormDrawer';

import { MAX_FILTER_NAME, MAX_RETENTION_VALUES } from './retention';
import { ValuesInput } from './RetentionSection';
import { MAX_SEGMENT_CONDITIONS, MAX_SEGMENTS, newSegment, newSegmentCondition } from './segments';

/** What the select says for a property of the person, whose key is typed beside it. */
const PROPERTY = '__property__';

/** The key a field is written under in the translations: the dots of its name are underscores. */
const keyOf = (field: string) => field.replaceAll('.', '_');

/**
 * The groups of sessions the analytics pages can be narrowed to, each told by what stays the same
 * while a session lasts: who ran it, and where.
 */
export function SegmentsSection({
  settings,
  onChange,
  disabled,
}: {
  settings: RumApplicationSettings;
  onChange: (settings: RumApplicationSettings) => void;
  disabled: boolean;
}) {
  const { t } = useTranslation('rum');
  const segments = settings.segments ?? [];

  return (
    <FormSection title={t('applications.drawer.segments.title')} description={t('applications.drawer.segments.description')}>
      {/* A disabled fieldset disables every control in it, the add and remove buttons too. */}
      <fieldset disabled={disabled} className="m-0 flex min-w-0 flex-col gap-4 border-0 p-0" data-rum-segments="">
        <FieldArray<RumSegment>
          items={segments}
          onChange={(next) => onChange({ ...settings, segments: next })}
          maxItems={MAX_SEGMENTS}
          newItem={() =>
            newSegment(
              segments.map((segment) => segment.name),
              (number) => t('applications.drawer.segments.new_name', { number }),
            )
          }
          addLabel={t('applications.drawer.segments.add')}
          removeLabel={t('applications.drawer.segments.remove')}
          emptyHint={t('applications.drawer.segments.none')}
          renderItem={(segment, index, setSegment) => (
            <SegmentCard
              segment={segment}
              repeated={segments.some(
                (other, at) => at !== index && other.name.trim().toLowerCase() === segment.name.trim().toLowerCase(),
              )}
              onChange={setSegment}
            />
          )}
        />
      </fieldset>
    </FormSection>
  );
}

function SegmentCard({
  segment,
  repeated,
  onChange,
}: {
  segment: RumSegment;
  /** Another segment has this name. */
  repeated: boolean;
  onChange: (segment: RumSegment) => void;
}) {
  const { t } = useTranslation('rum');
  const named = segment.name.trim() !== '' && !repeated;

  return (
    <div data-rum-segment-card="" className="flex min-w-0 flex-col gap-2.5 rounded-md bg-bg-2 p-3">
      <FormInput
        value={segment.name}
        maxLength={MAX_FILTER_NAME}
        onChange={(event) => onChange({ ...segment, name: event.target.value })}
        aria-label={t('applications.drawer.segments.name')}
        aria-invalid={!named || undefined}
        placeholder={t('applications.drawer.segments.name')}
      />
      <FieldArray<RumSegmentCondition>
        items={segment.conditions}
        onChange={(conditions) => onChange({ ...segment, conditions })}
        maxItems={MAX_SEGMENT_CONDITIONS}
        newItem={newSegmentCondition}
        addLabel={t('applications.drawer.retention.add_condition')}
        removeLabel={t('applications.drawer.retention.remove_condition')}
        renderItem={(condition, _index, setCondition) => <SegmentConditionRow condition={condition} onChange={setCondition} />}
      />
    </div>
  );
}

function SegmentConditionRow({
  condition,
  onChange,
}: {
  condition: RumSegmentCondition;
  onChange: (condition: RumSegmentCondition) => void;
}) {
  const { t } = useTranslation('rum');
  const property = condition.field.startsWith(SEGMENT_USER_PREFIX);

  return (
    <div data-rum-segment-condition="" className="grid min-w-0 grid-cols-[minmax(0,1fr)_6rem] items-center gap-2">
      <FormSelect
        value={property ? PROPERTY : condition.field}
        onChange={(field) => onChange({ ...condition, field: field === PROPERTY ? SEGMENT_USER_PREFIX : field })}
        ariaLabel={t('applications.drawer.retention.condition_field')}
        options={[
          ...SEGMENT_FIELDS.map((field) => ({ value: field, label: t(`applications.drawer.segments.fields.${keyOf(field)}`) })),
          { value: PROPERTY, label: t('applications.drawer.segments.fields.property') },
        ]}
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
      {property && (
        <div className="col-span-2">
          <FormInput
            value={condition.field.slice(SEGMENT_USER_PREFIX.length)}
            onChange={(event) => onChange({ ...condition, field: `${SEGMENT_USER_PREFIX}${event.target.value.trim()}` })}
            aria-label={t('applications.drawer.segments.property_key')}
            aria-invalid={condition.field === SEGMENT_USER_PREFIX || undefined}
            placeholder={t('applications.drawer.segments.property_key')}
          />
        </div>
      )}
      <div className="col-span-2">
        <ValuesInput
          values={condition.values}
          onChange={(values) => onChange({ ...condition, values: values.slice(0, MAX_RETENTION_VALUES) })}
          label={t('applications.drawer.retention.condition_values')}
          placeholder={t('applications.drawer.retention.values_placeholder')}
        />
      </div>
    </div>
  );
}
