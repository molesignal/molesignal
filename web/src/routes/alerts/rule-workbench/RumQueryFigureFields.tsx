import { ExternalLink } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumMeasureFn } from '@/api/rum';
import { parseRumStatement } from '@/routes/rum/explorer/fieldQuery';
import { RUM_TABS, type RumTab, rumTabOf } from '@/routes/rum/explorer/model';
import { useRumSchema } from '@/routes/rum/explorer/useRumSchema';
import { MEASURE_FNS, measureFields } from '@/routes/rum/explorer/viz/spec';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import type { RumAlertSearch } from '@/types/alerting';

import { searchUnit } from './rumSource';

/** The address of the explorer that draws what a rule made from it watches, over time. */
export function explorerHrefOf(search: RumAlertSearch): string {
  const params = new URLSearchParams({ type: search.type });

  if (search.statement?.trim()) params.set('q', search.statement.trim());
  params.set('viz', 'timeseries');
  params.set('m', search.measure.field ? `${search.measure.fn}:${search.measure.field}` : search.measure.fn);

  return `/rum/explorer?${params}`;
}

const same = (left: unknown, right: unknown) => JSON.stringify(left) === JSON.stringify(right);

/**
 * What a rule made from the explorer watches: the type of rows, the query as the explorer writes
 * it, and the number taken of the rows it matches. The query is read against the fields of the
 * type as the explorer reads it: what the type cannot answer is said, and left out.
 */
export function RumQueryFigureFields({ search, onChange }: { search: RumAlertSearch; onChange: (next: RumAlertSearch) => void }) {
  const { t } = useTranslation('alerts');
  const { t: tRum } = useTranslation('rum');
  const tab: RumTab = rumTabOf(search.type) ?? 'views';
  const schema = useRumSchema(tab);
  const fields = React.useMemo(() => schema.data?.fields ?? [], [schema.data]);
  const statement = search.statement ?? '';
  const parsed = React.useMemo(() => parseRumStatement(statement, fields), [fields, statement]);

  // Once the fields of the type are known, what is evaluated is what the query says of them.
  React.useEffect(() => {
    if (!schema.isSuccess) return;

    const filters = parsed.filters.map(({ field, op, value }) => ({ field, op, value }));

    if (!same(filters, search.filters ?? []) || !same(parsed.freeText, search.freeText ?? [])) {
      onChange({ ...search, filters, freeText: parsed.freeText });
    }
  }, [onChange, parsed, schema.isSuccess, search]);

  const fn = search.measure.fn as RumMeasureFn;
  const measurable = measureFields(fn, fields);
  const field = search.measure.field ?? '';
  const fieldUnit = fields.find((candidate) => candidate.name === field)?.unit;
  const fieldOptions = [...new Set([...(field ? [field] : []), ...measurable.map((candidate) => candidate.name)])];

  const setMeasure = (next: RumMeasureFn) => {
    const options = measureFields(next, fields);
    const kept = options.some((candidate) => candidate.name === field) ? field : options[0]?.name;

    onChange({ ...search, measure: { fn: next, ...(next !== 'count' && kept ? { field: kept } : {}) } });
  };

  return (
    <>
      <FormField label={t('workbench.rum.query.type')} required>
        <FormSelect
          ariaLabel={t('workbench.rum.query.type')}
          value={tab}
          onChange={(value) =>
            // Another type has other fields: the number starts again as a count of its rows.
            onChange({ ...search, type: value, filters: [], freeText: [], measure: { fn: 'count' } })
          }
          options={RUM_TABS.map((type) => ({ value: type, label: tRum(`explorer.types.${type}`) }))}
        />
      </FormField>
      <div data-alert-rum-measure="" className="grid grid-cols-2 gap-2">
        <FormField label={t('workbench.rum.query.measure')} hint={t(`workbench.rum.unit.${searchUnit(search, fieldUnit)}`)} required>
          <FormSelect
            ariaLabel={t('workbench.rum.query.measure')}
            value={fn}
            onChange={(value) => setMeasure(value as RumMeasureFn)}
            options={MEASURE_FNS.map((option) => ({ value: option, label: t(`workbench.rum.query.functions.${option}`) }))}
          />
        </FormField>
        {fn !== 'count' && (
          <FormField label={t('workbench.rum.query.field')} required>
            <FormSelect
              ariaLabel={t('workbench.rum.query.field')}
              value={field}
              onChange={(value) => onChange({ ...search, measure: { fn, field: value } })}
              options={fieldOptions}
              placeholder={t('workbench.rum.query.field_placeholder')}
              disabled={!schema.isSuccess}
            />
          </FormField>
        )}
      </div>
      <FormField
        label={t('workbench.rum.query.statement')}
        className="md:col-span-2"
        hint={
          parsed.rejected.length > 0
            ? t('workbench.rum.query.ignored', { parts: parsed.rejected.join(', ') })
            : t('workbench.rum.query.statement_hint')
        }
      >
        <div className="flex items-center gap-2">
          <FormInput
            data-alert-rum-statement=""
            aria-label={t('workbench.rum.query.statement')}
            className="min-w-0 flex-1 font-mono"
            value={statement}
            placeholder="browser.name = 'Chrome'"
            disabled={!schema.isSuccess}
            onChange={(event) => onChange({ ...search, statement: event.target.value })}
          />
          <Link
            to={explorerHrefOf(search)}
            data-alert-rum-explorer=""
            className="inline-flex h-8 shrink-0 items-center gap-1 rounded-md px-2 font-sans text-xs text-[var(--blue-ink)] hover:bg-bg-3 focus-visible:bg-bg-3"
          >
            {t('workbench.rum.query.open_explorer')}
            <ExternalLink aria-hidden className="h-3 w-3" />
          </Link>
        </div>
      </FormField>
    </>
  );
}
