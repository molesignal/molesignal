import { Plus, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ExtendTableSummary } from '@/api/extendTables';
import { ChromeButton, IconButton, uiLabelClass } from '@/shell/chrome';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';

import type { LookupField, ProcessorConfig } from './catalog';

type LookupConfig = Extract<ProcessorConfig, { op: 'lookup' }>;

const FIELD_INPUT = 'font-mono text-xs';

/**
 * The form of an extend table lookup: the table, the field of the event whose
 * value is the row's key, and which values of the row go into which fields of
 * the event. A row that is not there changes nothing.
 */
export function LookupFields({
  config,
  tables,
  onChange,
}: {
  config: LookupConfig;
  /** The workspace's extend tables; a table named before that is gone stays offered. */
  tables: readonly ExtendTableSummary[];
  onChange: (next: LookupConfig) => void;
}) {
  const { t } = useTranslation('pipelines');
  const table = tables.find((item) => item.tableName === config.table);
  const names = tables.map((item) => item.tableName);
  const options = config.table && !names.includes(config.table) ? [config.table, ...names] : names;
  const columns = table?.valueFields.map((field) => field.name) ?? [];
  const setField = (index: number, patch: Partial<LookupField>) =>
    onChange({
      ...config,
      fields: config.fields.map((item, at) => (at === index ? { ...item, ...patch } : item)),
    });

  return (
    <div className="grid gap-[12px]">
      <div className="grid gap-x-[16px] gap-y-[12px] sm:grid-cols-2 xl:grid-cols-3">
        <FormField
          label={t('processors.lookup_table')}
          hint={tables.length === 0 ? t('processors.lookup_no_tables') : t('processors.lookup_table_hint')}
        >
          <FormSelect
            value={config.table}
            onChange={(next) => onChange({ ...config, table: next })}
            options={options}
            placeholder={t('processors.lookup_table_placeholder')}
            ariaLabel={t('processors.lookup_table')}
          />
        </FormField>
        <FormField
          label={t('processors.lookup_key')}
          {...(table && { hint: t('processors.lookup_key_hint', { key: table.keyField }) })}
        >
          <FormInput
            value={config.key}
            onChange={(event) => onChange({ ...config, key: event.target.value })}
            placeholder="labels.host"
            className={FIELD_INPUT}
            autoComplete="off"
            spellCheck={false}
          />
        </FormField>
      </div>

      <div className="grid gap-[6px]" role="group" aria-label={t('processors.lookup_fields')}>
        <span className={uiLabelClass}>{t('processors.lookup_fields')}</span>
        {config.fields.map((mapping, index) => (
          <div key={index} data-lookup-field className="flex min-w-0 items-center gap-[8px]">
            {columns.length > 0 ? (
              <FormSelect
                value={mapping.column}
                onChange={(column) => setField(index, { column })}
                options={mapping.column && !columns.includes(mapping.column) ? [mapping.column, ...columns] : columns}
                placeholder={t('processors.lookup_column')}
                ariaLabel={t('processors.lookup_column')}
                className="w-[180px]"
              />
            ) : (
              <FormInput
                value={mapping.column}
                onChange={(event) => setField(index, { column: event.target.value })}
                placeholder={t('processors.lookup_column')}
                aria-label={t('processors.lookup_column')}
                className={`w-[180px] ${FIELD_INPUT}`}
                autoComplete="off"
                spellCheck={false}
              />
            )}
            <span aria-hidden className="text-tx-3">
              →
            </span>
            <FormInput
              value={mapping.field}
              onChange={(event) => setField(index, { field: event.target.value })}
              placeholder="labels.owner"
              aria-label={t('processors.lookup_into')}
              className={`min-w-0 flex-1 ${FIELD_INPUT}`}
              autoComplete="off"
              spellCheck={false}
            />
            <IconButton
              aria-label={t('processors.lookup_remove_field')}
              disabled={config.fields.length === 1}
              onClick={() => onChange({ ...config, fields: config.fields.filter((_, at) => at !== index) })}
            >
              <X aria-hidden className="h-3.5 w-3.5" />
            </IconButton>
          </div>
        ))}
        <ChromeButton
          size="sm"
          variant="ghost"
          className="justify-self-start text-tx-2"
          onClick={() => onChange({ ...config, fields: [...config.fields, { column: '', field: '' }] })}
        >
          <Plus aria-hidden className="h-3.5 w-3.5" />
          {t('processors.lookup_add_field')}
        </ChromeButton>
      </div>
    </div>
  );
}
