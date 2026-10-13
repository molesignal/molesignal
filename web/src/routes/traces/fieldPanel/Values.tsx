import { Minus, Plus } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { useTimeFormatter } from '@/lib/time';
import { FieldValueList, type FieldValueAction, type FieldValueItem } from '@/shell/query/FieldValues';
import { Skeleton } from '@/shell/ui/skeleton';

import type { TraceFieldDef, TraceQueryMode } from '../fieldQueryModel';
import { canFilterTraceValue, type TraceValueOperator } from '../valueFilter';
import {
  describeTraceValue,
  isTraceFieldMeasure,
  isTraceTimeKind,
  traceValueKind,
  type TraceFieldRecord,
  type TraceFieldValues,
} from './model';
import { type TraceFieldValueSource, useTraceFieldValues } from './valuesQuery';

interface TraceFieldValuesBlockProps {
  field: TraceFieldDef;
  traces: readonly TraceFieldRecord[];
  source: TraceFieldValueSource;
  /** The language a filter would be written in: it decides which filters can be offered. */
  filterMode: TraceQueryMode;
  /** Whether the query already holds this filter; the button that added it then takes it out. */
  isValueFiltered: (field: TraceFieldDef, value: string, op: TraceValueOperator) => boolean;
  onFilterValue: (field: TraceFieldDef, value: string, op: TraceValueOperator) => void;
}

const ACTION: Record<TraceValueOperator, { icon: FieldValueAction['icon']; hint: string; tone?: 'remove' }> = {
  '=': { icon: Plus, hint: 'filter_for' },
  '!=': { icon: Minus, hint: 'filter_out', tone: 'remove' },
  '>=': { icon: '≥', hint: 'filter_at_least' },
  '<=': { icon: '≤', hint: 'filter_at_most' },
};

/**
 * What the loaded traces hold for a field, under its row: the most common
 * values with the traces they are in, or for a measure what it runs from and
 * to. Every value can be turned into a filter where the language can say it.
 */
export function TraceFieldValuesBlock({
  field,
  traces,
  source,
  filterMode,
  isValueFiltered,
  onFilterValue,
}: TraceFieldValuesBlockProps) {
  const { t } = useTranslation('traces');
  const { t: tCommon } = useTranslation('common');
  const format = useTimeFormatter();
  const { values, loading, failed, retry } = useTraceFieldValues(field, traces, source);

  if (loading) {
    return (
      <div role="status" aria-label={t('explore.labels.values_loading', { name: field.name })} className="flex flex-col gap-[6px] px-[6px] py-[6px]">
        {['w-3/4', 'w-1/2', 'w-2/3'].map((width) => (
          <Skeleton key={width} className={`h-[16px] ${width}`} />
        ))}
      </div>
    );
  }
  if (failed) {
    return (
      <p role="alert" className="flex flex-wrap items-center gap-x-2 px-[6px] py-[6px] font-sans text-xs text-tx-2">
        {t('explore.labels.values_failed')}
        <button type="button" onClick={retry} className="rounded font-strong text-[var(--blue-ink)] hover:underline focus-visible:underline">
          {tCommon('actions.retry')}
        </button>
      </p>
    );
  }
  if (!values || values.top.length === 0) {
    return (
      <p className="px-[6px] py-[6px] font-sans text-xs text-tx-2">
        {t(traces.length === 0 ? 'explore.labels.no_traces' : 'explore.labels.no_values')}
      </p>
    );
  }

  const kind = traceValueKind(field);
  const describe = (value: string) => describeTraceValue(kind, value, (micros) => format.micros(micros));
  const actionsFor = (value: string, operators: TraceValueOperator[]): FieldValueAction[] =>
    operators
      .filter((op) => canFilterTraceValue(field, value, op, filterMode))
      .map((op) => {
        const pressed = isValueFiltered(field, value, op);

        return {
          id: op,
          icon: ACTION[op].icon,
          hint: t(pressed ? 'explore.labels.filter_remove' : `explore.labels.${ACTION[op].hint}`),
          label: t(`explore.labels.${ACTION[op].hint}_aria`, { name: field.name, value: describe(value) }),
          pressed,
          ...(ACTION[op].tone ? { tone: ACTION[op].tone } : {}),
          onSelect: () => onFilterValue(field, value, op),
        };
      });

  const measureItems = (measured: TraceFieldValues): FieldValueItem[] => {
    const range = measured.range!;
    // A moment has an earliest and a latest; an average of moments says nothing.
    const rows = isTraceTimeKind(kind)
      ? ([['range_earliest', range.min], ['range_latest', range.max]] as const)
      : ([['range_min', range.min], ['range_avg', range.avg], ['range_max', range.max]] as const);

    return rows.map(([caption, value]) => ({
      key: caption,
      caption: t(`explore.labels.${caption}`),
      label: describe(value),
      title: value,
      actions: actionsFor(value, ['>=', '<=']),
    }));
  };
  const measure = isTraceFieldMeasure(values);
  // Values that are each in one trace of many (ids) have no distribution to draw, and a "1"
  // on every row says nothing: the room goes to the value.
  const even = traces.length > 1 && values.top.every((item) => item.traces === 1);
  const items: FieldValueItem[] = measure
    ? measureItems(values)
    : values.top.map((item) => ({
        key: item.value,
        label: describe(item.value),
        title: item.value,
        ...(even
          ? {}
          : {
              count: item.traces,
              countTitle: [
                t('explore.labels.value_trace_count', { count: item.traces }),
                ...(item.spans === undefined ? [] : [t('explore.labels.value_span_count', { count: item.spans })]),
              ].join(' · '),
              ...(traces.length > 0 ? { share: item.traces / traces.length } : {}),
            }),
        actions: actionsFor(item.value, ['=', '!=']),
      }));
  const shown = values.top.length;

  return (
    <>
      <p data-trace-field-summary className="px-[6px] pb-[4px] pt-[2px] font-sans text-xs text-tx-3">
        {measure || values.distinct <= shown
          ? t('explore.labels.values_count', { count: values.distinct })
          : t('explore.labels.values_top', { shown, count: values.distinct })}
        {' · '}
        {t('explore.labels.values_in_traces', { count: traces.length, matched: values.traces })}
      </p>
      <FieldValueList items={items} />
    </>
  );
}
