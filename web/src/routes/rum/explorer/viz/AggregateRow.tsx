import { Plus, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumField } from '@/api/rum';
import { FittedSelect } from '@/shell/ui/fitted-select';

import type { RumVizAddress } from './address';
import { CHIP, FieldPicker, GHOST } from './FieldPicker';
import { bucketText } from './format';
import { MeasurePicker } from './MeasurePicker';
import {
  autoBucketOf,
  bucketChoices,
  COUNTRY_FIELD,
  groupFields,
  MAX_GROUPS,
  MAX_MEASURES,
  numberFields,
  type RumMeasureChoice,
  type RumVizSpec,
  measureToken,
  tokenOfBucket,
  topChoices,
} from './spec';

const SELECT =
  'h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs text-tx-0 hover:bg-bg-3 focus-visible:bg-bg-3 data-[state=open]:bg-bg-3';
const REMOVE =
  'grid h-8 w-6 shrink-0 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none';
const AUTO = 'auto';

/**
 * The row under the query that says what is counted of the rows it matches: the number (a
 * function of a field), the groups the rows are told apart in, how long a bar of time is, and how
 * many groups are drawn. Each change is made at once, and written to the address.
 */
export function RumAggregateRow({
  spec,
  fields,
  range,
  onChange,
}: {
  spec: RumVizSpec;
  fields: readonly RumField[];
  /** The range the numbers are of, in microseconds. */
  range: { from: number; to: number };
  onChange: (patch: Partial<RumVizAddress>) => void;
}) {
  const { t, i18n } = useTranslation('rum');
  const language = i18n.resolvedLanguage ?? i18n.language;
  const { viz } = spec;
  const groupable = React.useMemo(() => groupFields(fields), [fields]);
  const lengths = React.useMemo(() => bucketChoices({ start: range.from, end: range.to }), [range.from, range.to]);
  const auto = React.useMemo(() => autoBucketOf({ start: range.from, end: range.to }), [range.from, range.to]);

  const writeMeasures = (measures: readonly RumMeasureChoice[]) => {
    const tokens = measures.map(measureToken);
    // A count alone is what an address without measures means.
    onChange({ measures: tokens.length === 1 && tokens[0] === 'count' ? [] : tokens });
  };
  const measureAt = (index: number, next: RumMeasureChoice) =>
    writeMeasures(spec.measures.map((measure, at) => (at === index ? next : measure)));

  // A chart can stand for the whole; the others are of some groups.
  const fewest = viz === 'timeseries' ? 0 : 1;
  const groupAt = (index: number, name: string) => onChange({ groups: spec.groups.map((group, at) => (at === index ? name : group)) });
  const ungroup = (index: number) => onChange({ groups: spec.groups.filter((_, at) => at !== index) });

  const bucket = spec.bucket !== null && lengths.includes(spec.bucket) ? tokenOfBucket(spec.bucket) : null;
  const bucketOptions = [
    {
      value: AUTO,
      label: t('explorer.viz.bucket_auto_of', { bucket: bucketText(auto, language) }),
    },
    ...lengths.flatMap((micros) => {
      const token = tokenOfBucket(micros);
      return token ? [{ value: token, label: bucketText(micros, language) }] : [];
    }),
  ];

  // A distribution is of one number: there is nothing to count it by, and nothing to group.
  if (viz === 'distribution') {
    return (
      <div data-rum-aggregate={viz} role="group" aria-label={t('explorer.viz.aggregate')} className="flex flex-wrap items-center gap-[6px]">
        <FieldPicker
          fields={numberFields(fields)}
          value={spec.field}
          prefix={t('explorer.viz.field')}
          label={t('explorer.viz.field_edit', { field: spec.field ?? '' })}
          testId="field"
          onSelect={(name) => onChange({ field: name })}
        />
      </div>
    );
  }

  return (
    <div data-rum-aggregate={viz} role="group" aria-label={t('explorer.viz.aggregate')} className="flex flex-wrap items-center gap-[6px]">
      {spec.measures.map((measure, index) => (
        <React.Fragment key={`${measure.fn}:${measure.field ?? ''}`}>
          <MeasurePicker
            measure={measure}
            fields={fields}
            // Said once: the measures after the first are more of the same.
            {...(index === 0 ? { prefix: t('explorer.viz.measure') } : {})}
            label={t('explorer.viz.measure_edit', { measure: measureToken(measure) })}
            testId="measure"
            onSelect={(next) => measureAt(index, next)}
          />
          {spec.measures.length > 1 && (
            <button
              type="button"
              aria-label={t('explorer.viz.measure_remove', { measure: measureToken(measure) })}
              className={REMOVE}
              onClick={() => writeMeasures(spec.measures.filter((_, at) => at !== index))}
            >
              <X aria-hidden="true" className="h-3 w-3" />
            </button>
          )}
        </React.Fragment>
      ))}
      {spec.measures.length < MAX_MEASURES[viz] && (
        <MeasurePicker
          measure={null}
          fields={fields}
          label={t('explorer.viz.measure_add')}
          testId="measure-add"
          onSelect={(next) => writeMeasures([...spec.measures, next])}
          trigger={
            <button type="button" aria-label={t('explorer.viz.measure_add')} data-rum-viz-control="measure-add" className={GHOST}>
              <Plus aria-hidden="true" className="h-3 w-3" />
              {t('explorer.viz.measure')}
            </button>
          }
        />
      )}

      {viz === 'map' ? (
        <span data-rum-viz-control="group-fixed" title={t('explorer.viz.map_group')} className={CHIP}>
          <span className="shrink-0 text-tx-3">{t('explorer.viz.group')}</span>
          <span className="font-mono">{COUNTRY_FIELD}</span>
        </span>
      ) : (
        <>
          {spec.groups.map((group, index) => (
            <React.Fragment key={group}>
              <FieldPicker
                // The other groups are not offered again: a field is grouped by once.
                fields={groupable.filter((field) => field.name === group || !spec.groups.includes(field.name))}
                value={group}
                prefix={index === 0 ? t('explorer.viz.group') : t('explorer.viz.group_then')}
                label={t('explorer.viz.group_edit', { field: group })}
                testId="group"
                onSelect={(name) => groupAt(index, name)}
              />
              {spec.groups.length > fewest && (
                <button type="button" aria-label={t('explorer.viz.group_remove', { field: group })} className={REMOVE} onClick={() => ungroup(index)}>
                  <X aria-hidden="true" className="h-3 w-3" />
                </button>
              )}
            </React.Fragment>
          ))}
          {spec.groups.length < MAX_GROUPS[viz] && (
            <FieldPicker
              fields={groupable.filter((field) => !spec.groups.includes(field.name))}
              value={null}
              label={t('explorer.viz.group_add')}
              testId="group-add"
              onSelect={(name) => onChange({ groups: [...spec.groups, name] })}
              trigger={
                <button type="button" aria-label={t('explorer.viz.group_add')} data-rum-viz-control="group-add" className={GHOST}>
                  <Plus aria-hidden="true" className="h-3 w-3" />
                  {spec.groups.length === 0 ? t('explorer.viz.group') : t('explorer.viz.group_then')}
                </button>
              }
            />
          )}
        </>
      )}

      {viz === 'timeseries' && (
        <FittedSelect
          aria-label={t('explorer.viz.bucket')}
          data-rum-viz-control="bucket"
          value={bucket ?? AUTO}
          onValueChange={(value) => onChange({ bucket: value === AUTO ? null : value })}
          options={bucketOptions}
          maxWidth={220}
          prefix={t('explorer.viz.bucket')}
          className={SELECT}
        />
      )}
      {viz !== 'map' && (
        <FittedSelect
          aria-label={t('explorer.viz.top')}
          data-rum-viz-control="top"
          value={String(spec.top)}
          onValueChange={(value) => onChange({ top: Number(value) })}
          options={topChoices(viz).map((count) => ({ value: String(count), label: String(count) }))}
          maxWidth={120}
          prefix={t('explorer.viz.top')}
          className={SELECT}
        />
      )}
    </div>
  );
}
