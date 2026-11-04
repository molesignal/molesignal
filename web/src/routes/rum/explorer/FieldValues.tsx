import { useQuery } from '@tanstack/react-query';
import { Minus, Plus } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import type { RumField } from '@/api/rum';
import { FieldValueList, type FieldValueAction, type FieldValueItem } from '@/shell/query/FieldValues';
import { Skeleton } from '@/shell/ui/skeleton';

import { describeCell } from './cells';
import type { ParsedRumStatement } from './fieldQuery';
import type { RumTab } from './model';
import { canFilterRumValue, type RumValueOperator } from './valueFilter';

/** Where the values of a field are read from: the rows the applied query matches. */
export interface RumValueSource {
  orgId: string;
  tab: RumTab;
  /** Microseconds. */
  range: { from: number; to: number };
  parsed: ParsedRumStatement;
  /** Goes up when the reader asks for the results again. */
  epoch: number;
}

const VALUES_SHOWN = 10;

const ACTION: Record<'=' | '!=', { icon: FieldValueAction['icon']; hint: 'filter_for' | 'filter_out'; tone?: 'remove' }> = {
  '=': { icon: Plus, hint: 'filter_for' },
  '!=': { icon: Minus, hint: 'filter_out', tone: 'remove' },
};

interface RumFieldValuesBlockProps {
  field: RumField;
  source: RumValueSource;
  language: string;
  /** Whether the query already holds this filter; the button that added it then takes it out. */
  isValueFiltered: (field: RumField, value: string, op: RumValueOperator) => boolean;
  onFilterValue: (field: RumField, value: string, op: RumValueOperator) => void;
}

/**
 * The commonest values of a field across the rows the query matches, under its row: each with
 * how common it is next to the commonest, and the two ways to turn it into a filter.
 */
export function RumFieldValuesBlock({
  field,
  source,
  language,
  isValueFiltered,
  onFilterValue,
}: RumFieldValuesBlockProps) {
  const { t } = useTranslation('rum');
  const { t: tCommon } = useTranslation('common');
  const { orgId, tab, range, parsed, epoch } = source;

  const query = useQuery({
    queryKey: ['rum', 'facets', orgId, tab, field.name, range.from, range.to, JSON.stringify(parsed), epoch],
    queryFn: ({ signal }) =>
      rumApi.facets(
        {
          type: tab,
          field: field.name,
          from: range.from,
          to: range.to,
          limit: VALUES_SHOWN,
          ...(parsed.filters.length > 0 ? { filters: parsed.filters } : {}),
          ...(parsed.freeText.length > 0 ? { freeText: parsed.freeText } : {}),
        },
        signal,
      ),
    staleTime: 30_000,
  });

  if (query.isLoading) {
    return (
      <div
        role="status"
        aria-label={t('explorer.fields.values_loading', { name: field.name })}
        className="flex flex-col gap-[6px] px-[6px] py-[6px]"
      >
        {['w-3/4', 'w-1/2', 'w-2/3'].map((width) => (
          <Skeleton key={width} className={`h-[16px] ${width}`} />
        ))}
      </div>
    );
  }

  if (query.isError) {
    return (
      <p role="alert" className="flex flex-wrap items-center gap-x-2 px-[6px] py-[6px] font-sans text-xs text-tx-2">
        {t('explorer.fields.values_failed')}
        <button
          type="button"
          onClick={() => void query.refetch()}
          className="rounded font-strong text-[var(--blue-ink)] hover:underline focus-visible:underline"
        >
          {tCommon('actions.retry')}
        </button>
      </p>
    );
  }

  const answer = query.data;

  if (answer?.unavailable) {
    return (
      <p className="px-[6px] py-[6px] font-sans text-xs text-tx-2">
        {t(`explorer.fields.unavailable.${answer.unavailable}`)}
      </p>
    );
  }

  const values = answer?.values ?? [];

  if (values.length === 0) {
    return <p className="px-[6px] py-[6px] font-sans text-xs text-tx-2">{t('explorer.fields.no_values')}</p>;
  }

  const describe = (value: string) => {
    const cell = describeCell(field, numberOf(field, value), language);
    return cell?.text ?? value;
  };
  const top = Math.max(...values.map((item) => item.count));
  // Values that are each in one row (ids) have no distribution to draw, and a "1" on every row
  // says nothing: the room goes to the value.
  const even = values.every((item) => item.count === 1);

  const actionsFor = (value: string): FieldValueAction[] =>
    (['=', '!='] as const)
      .filter((op) => canFilterRumValue(field, value, op))
      .map((op) => {
        const pressed = isValueFiltered(field, value, op);

        return {
          id: op,
          icon: ACTION[op].icon,
          hint: t(pressed ? 'explorer.fields.filter_remove' : `explorer.fields.${ACTION[op].hint}`),
          label: t(`explorer.fields.${ACTION[op].hint}_aria`, { name: field.name, value: describe(value) }),
          pressed,
          ...(ACTION[op].tone ? { tone: ACTION[op].tone } : {}),
          onSelect: () => onFilterValue(field, value, op),
        };
      });

  const items: FieldValueItem[] = values.map((item) => ({
    key: item.value,
    label: describe(item.value),
    title: item.value,
    ...(even ? {} : { count: item.count, share: item.count / top }),
    actions: actionsFor(item.value),
  }));

  return (
    <>
      <p data-rum-field-summary className="px-[6px] pb-[4px] pt-[2px] font-sans text-xs text-tx-3">
        {t('explorer.fields.values_top', { shown: values.length })}
      </p>
      <FieldValueList items={items} />
    </>
  );
}

/** The value of a counted field as the cell shows it: a number stays a number. */
function numberOf(field: RumField, value: string): unknown {
  if (field.dataType === 'int64' || field.dataType === 'float64') {
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : value;
  }

  return field.dataType === 'bool' ? value === 'true' : value;
}
