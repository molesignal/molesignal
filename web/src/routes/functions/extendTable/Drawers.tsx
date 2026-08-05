import { useMutation, useQueryClient } from '@tanstack/react-query';
import {
  AlertTriangle,
  Check,
  ChevronLeft,
  FileJson2,
  FileSpreadsheet,
  type LucideIcon,
  Plus,
  TableProperties,
  Trash2,
  Upload,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as extendTablesApi from '@/api/extendTables';
import { toApiError } from '@/lib/http';
import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Checkbox } from '@/shell/ui/checkbox';
import { toast } from '@/shell/ui/sonner';

import {
  displayValue,
  parseFieldValue,
  parseImportText,
  type ImportRecord,
  valueAsObject,
} from './model';

const EMPTY_FIELD: extendTablesApi.ExtendValueField = {
  name: '',
  field_type: 'string',
  required: false,
  description: '',
};

export function CreateExtendTableDrawer({
  access,
  open,
  onClose,
  onCreated,
}: {
  access: ActionAccess;
  open: boolean;
  onClose: () => void;
  onCreated: (table: extendTablesApi.ExtendTableSummary) => void;
}) {
  const { t } = useTranslation('functions');
  const qc = useQueryClient();
  const [step, setStep] = React.useState<1 | 2>(1);
  const [tableName, setTableName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [keyField, setKeyField] = React.useState('key');
  const [fields, setFields] = React.useState<extendTablesApi.ExtendValueField[]>([
    { ...EMPTY_FIELD },
  ]);
  const [startMode, setStartMode] = React.useState<'empty' | 'paste'>('empty');
  const [importText, setImportText] = React.useState('');

  React.useEffect(() => {
    if (!open) return;
    setStep(1);
    setTableName('');
    setDescription('');
    setKeyField('key');
    setFields([{ ...EMPTY_FIELD }]);
    setStartMode('empty');
    setImportText('');
  }, [open]);

  const effectiveFields = fields.filter((field) => field.name.trim());
  const duplicateFields =
    new Set(effectiveFields.map((field) => field.name.trim())).size !== effectiveFields.length;
  const definitionValid = Boolean(tableName.trim() && keyField.trim() && !duplicateFields);
  const parsedImport = React.useMemo(() => {
    if (startMode === 'empty' || !importText.trim()) {
      return { records: [] as ImportRecord[], error: '' };
    }
    try {
      return { records: parseImportText(importText, keyField.trim()), error: '' };
    } catch {
      return { records: [] as ImportRecord[], error: t('extend_tables.import_invalid') };
    }
  }, [importText, keyField, startMode, t]);

  const create = useMutation({
    mutationFn: async () => {
      const table = await extendTablesApi.createTable({
        table_name: tableName.trim(),
        description: description.trim(),
        key_field: keyField.trim(),
        value_fields: effectiveFields.map((field) => ({
          ...field,
          name: field.name.trim(),
          description: field.description.trim(),
        })),
      });
      for (const record of parsedImport.records) {
        await extendTablesApi.upsert(table.table_name, record.key, record.value);
      }
      return {
        ...table,
        row_count: parsedImport.records.length,
      };
    },
    onSuccess: async (table) => {
      await qc.invalidateQueries({ queryKey: ['extend-tables'] });
      toast.success(t('extend_tables.toast_table_created'));
      onCreated(table);
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const updateField = (
    index: number,
    patch: Partial<extendTablesApi.ExtendValueField>,
  ) => {
    setFields((current) =>
      current.map((field, fieldIndex) =>
        fieldIndex === index ? { ...field, ...patch } : field,
      ),
    );
  };

  const footer =
    step === 1 ? (
      <>
        <ChromeButton onClick={onClose}>{t('extend_tables.cancel')}</ChromeButton>
        <ChromeButton
          variant="primary"
          disabled={access.disabled || !definitionValid}
          disabledReason={access.reason}
          onClick={() => access.allowed && setStep(2)}
        >
          {t('extend_tables.continue')}
        </ChromeButton>
      </>
    ) : (
      <>
        <ChromeButton onClick={() => setStep(1)}>
          <ChevronLeft className="h-3.5 w-3.5" />
          {t('extend_tables.previous')}
        </ChromeButton>
        <ChromeButton onClick={onClose}>{t('extend_tables.cancel')}</ChromeButton>
        <ChromeButton
          variant="primary"
          disabled={
            access.disabled ||
            create.isPending ||
            (startMode === 'paste' &&
              (!importText.trim() || Boolean(parsedImport.error)))
          }
          disabledReason={access.reason}
          onClick={() => access.allowed && create.mutate()}
        >
          {create.isPending
            ? t('extend_tables.creating')
            : t('extend_tables.create_table')}
        </ChromeButton>
      </>
    );

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('extend_tables.create_title')}
      subtitle={t('extend_tables.create_subtitle')}
      width={720}
      footer={footer}
    >
      <fieldset
        disabled={access.disabled || create.isPending}
        aria-disabled={access.disabled || undefined}
        className="contents disabled:cursor-not-allowed"
      >
        <StepIndicator step={step} />
        {step === 1 ? (
        <div className="mt-6">
          <FormSection
            title={t('extend_tables.definition_title')}
            description={t('extend_tables.definition_description')}
          >
            <FormField label={t('extend_tables.field_table')} required>
              <FormInput
                autoFocus
                value={tableName}
                onChange={(event) => setTableName(event.target.value)}
                placeholder={t('extend_tables.table_name_placeholder')}
              />
            </FormField>
            <FormField label={t('extend_tables.field_description')}>
              <FormTextarea
                className="min-h-20"
                value={description}
                onChange={(event) => setDescription(event.target.value)}
                placeholder={t('extend_tables.description_placeholder')}
              />
            </FormField>
            <FormField
              label={t('extend_tables.key_field')}
              hint={t('extend_tables.key_field_hint')}
              required
            >
              <FormInput
                value={keyField}
                onChange={(event) => setKeyField(event.target.value)}
                placeholder="customer_id"
              />
            </FormField>
          </FormSection>
          <FormSection
            title={t('extend_tables.value_fields')}
            description={t('extend_tables.value_fields_hint')}
          >
            <div className="overflow-hidden rounded-lg border border-bd-0">
              {fields.map((field, index) => (
                <div
                  key={index}
                  className="border-b border-bd-0 bg-bg-1 p-4 last:border-b-0"
                >
                  <div className="grid gap-3 sm:grid-cols-[minmax(0,1.2fr)_150px_auto]">
                    <FormInput
                      aria-label={t('extend_tables.field_name')}
                      value={field.name}
                      onChange={(event) =>
                        updateField(index, { name: event.target.value })
                      }
                      placeholder={t('extend_tables.field_name_placeholder')}
                    />
                    <FormSelect
                      value={field.field_type}
                      onChange={(fieldType) =>
                        updateField(index, {
                          field_type: fieldType as extendTablesApi.ExtendFieldType,
                        })
                      }
                      options={[
                        { value: 'string', label: t('extend_tables.types.string') },
                        { value: 'number', label: t('extend_tables.types.number') },
                        { value: 'boolean', label: t('extend_tables.types.boolean') },
                        { value: 'object', label: t('extend_tables.types.object') },
                      ]}
                    />
                    <button
                      type="button"
                      onClick={() =>
                        setFields((current) =>
                          current.length === 1
                            ? [{ ...EMPTY_FIELD }]
                            : current.filter((_, fieldIndex) => fieldIndex !== index),
                        )
                      }
                      className="grid h-9 w-9 place-items-center rounded-md text-tx-3 hover:bg-red-dim hover:text-red"
                      aria-label={t('extend_tables.remove_field')}
                    >
                      <Trash2 className="h-4 w-4" />
                    </button>
                  </div>
                  <div className="mt-3 grid items-center gap-3 sm:grid-cols-[1fr_auto]">
                    <FormInput
                      aria-label={t('extend_tables.field_description')}
                      value={field.description}
                      onChange={(event) =>
                        updateField(index, { description: event.target.value })
                      }
                      placeholder={t('extend_tables.field_description_placeholder')}
                    />
                    <label className="flex min-h-9 items-center gap-2 text-xs text-tx-1">
                      <Checkbox
                        checked={field.required}
                        onCheckedChange={(checked) =>
                          updateField(index, { required: checked === true })
                        }
                      />
                      {t('extend_tables.required')}
                    </label>
                  </div>
                </div>
              ))}
            </div>
            {duplicateFields && (
              <InlineNotice tone="warning">
                {t('extend_tables.duplicate_fields')}
              </InlineNotice>
            )}
            <ChromeButton
              size="sm"
              className="self-start"
              onClick={() =>
                setFields((current) => [...current, { ...EMPTY_FIELD }])
              }
            >
              <Plus className="h-3.5 w-3.5" />
              {t('extend_tables.add_value_field')}
            </ChromeButton>
          </FormSection>
        </div>
      ) : (
        <div className="mt-6">
          <FormSection
            title={t('extend_tables.initial_data_title')}
            description={t('extend_tables.initial_data_description')}
          >
            <div className="grid gap-3 sm:grid-cols-2">
              <StartModeCard
                active={startMode === 'empty'}
                icon={TableProperties}
                title={t('extend_tables.start_empty')}
                description={t('extend_tables.start_empty_description')}
                onClick={() => setStartMode('empty')}
              />
              <StartModeCard
                active={startMode === 'paste'}
                icon={FileJson2}
                title={t('extend_tables.start_paste')}
                description={t('extend_tables.start_paste_description')}
                onClick={() => setStartMode('paste')}
              />
            </div>
            {startMode === 'paste' && (
              <FormField
                label={t('extend_tables.import_payload')}
                hint={t('extend_tables.import_payload_hint', { key: keyField })}
                required
              >
                <FormTextarea
                  className="min-h-64 font-mono text-xs"
                  value={importText}
                  onChange={(event) => setImportText(event.target.value)}
                  placeholder={`[\n  { "${keyField}": "customer-1001", "tier": "pro" }\n]`}
                />
              </FormField>
            )}
            {startMode === 'paste' && parsedImport.error && (
              <InlineNotice tone="warning">{parsedImport.error}</InlineNotice>
            )}
            {startMode === 'paste' &&
              !parsedImport.error &&
              parsedImport.records.length > 0 && (
                <InlineNotice tone="success">
                  {t('extend_tables.import_ready', {
                    count: parsedImport.records.length,
                  })}
                </InlineNotice>
              )}
          </FormSection>
        </div>
        )}
      </fieldset>
    </FormDrawer>
  );
}

export function UpsertExtendRowDrawer({
  access,
  open,
  table,
  rows,
  editingRow,
  onClose,
}: {
  access: ActionAccess;
  open: boolean;
  table: extendTablesApi.ExtendTableSummary;
  rows: extendTablesApi.ExtendRow[];
  editingRow: extendTablesApi.ExtendRow | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('functions');
  const qc = useQueryClient();
  const [mode, setMode] = React.useState<'form' | 'json'>('form');
  const [key, setKey] = React.useState('');
  const [values, setValues] = React.useState<Record<string, string>>({});
  const [json, setJson] = React.useState('{}');
  const [validationError, setValidationError] = React.useState('');

  React.useEffect(() => {
    if (!open) return;
    const object = valueAsObject(editingRow?.value_json ?? {});
    setKey(editingRow?.key ?? '');
    setValues(
      Object.fromEntries(
        table.value_fields.map((field) => [
          field.name,
          displayValue(object[field.name]) === '—'
            ? ''
            : displayValue(object[field.name]),
        ]),
      ),
    );
    setJson(JSON.stringify(object, null, 2));
    setMode(table.value_fields.length > 0 ? 'form' : 'json');
    setValidationError('');
  }, [editingRow, open, table.value_fields]);

  const existing = rows.find((row) => row.key === key.trim());
  const willOverwrite = Boolean(existing && existing.key !== editingRow?.key);

  const save = useMutation({
    mutationFn: () => {
      const trimmedKey = key.trim();
      if (!trimmedKey) throw new Error(t('extend_tables.key_required'));
      let value: Record<string, unknown>;
      if (mode === 'json') {
        const parsed = JSON.parse(json) as unknown;
        if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
          throw new Error(t('extend_tables.value_object_required'));
        }
        value = parsed as Record<string, unknown>;
      } else {
        value = {};
        for (const field of table.value_fields) {
          const raw = values[field.name] ?? '';
          if (field.required && !raw.trim()) {
            throw new Error(
              t('extend_tables.required_field_missing', { field: field.name }),
            );
          }
          if (raw.trim()) value[field.name] = parseFieldValue(raw, field.field_type);
        }
      }
      return extendTablesApi.upsert(table.table_name, trimmedKey, value);
    },
    onSuccess: async () => {
      await Promise.all([
        qc.invalidateQueries({
          queryKey: ['extend-tables', 'rows', table.table_name],
        }),
        qc.invalidateQueries({ queryKey: ['extend-tables', 'list'] }),
      ]);
      toast.success(
        editingRow
          ? t('extend_tables.toast_record_updated')
          : t('extend_tables.toast_record_added'),
      );
      onClose();
    },
    onError: (error) => {
      const message =
        error instanceof SyntaxError
          ? t('extend_tables.value_json_invalid')
          : error instanceof Error
            ? error.message
            : toApiError(error).message;
      setValidationError(message);
    },
  });

  const footer = (
    <>
      <ChromeButton onClick={onClose}>{t('extend_tables.cancel')}</ChromeButton>
      <ChromeButton
        variant="primary"
        disabled={access.disabled || save.isPending || !key.trim()}
        disabledReason={access.reason}
        onClick={() => {
          if (!access.allowed) return;
          setValidationError('');
          save.mutate();
        }}
      >
        {save.isPending
          ? t('extend_tables.saving')
          : editingRow
            ? t('extend_tables.save_changes')
            : t('extend_tables.add_record')}
      </ChromeButton>
    </>
  );

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={
        editingRow
          ? t('extend_tables.edit_record_title', { key: editingRow.key })
          : t('extend_tables.add_record_title')
      }
      subtitle={table.table_name}
      width={640}
      footer={footer}
    >
      <fieldset
        disabled={access.disabled || save.isPending}
        aria-disabled={access.disabled || undefined}
        className="contents disabled:cursor-not-allowed"
      >
      <FormSection>
        <FormField
          label={t('extend_tables.query_key_label', { key: table.key_field })}
          required
        >
          <FormInput
            autoFocus={!editingRow}
            value={key}
            disabled={Boolean(editingRow)}
            onChange={(event) => setKey(event.target.value)}
            placeholder={t('extend_tables.key_placeholder')}
          />
        </FormField>
      </FormSection>
      </fieldset>
      <FormSection
        title={t('extend_tables.field_values')}
        description={t('extend_tables.field_values_description')}
      >
        <SegmentedMode
          value={mode}
          onChange={setMode}
          options={[
            { value: 'form', label: t('extend_tables.form_mode') },
            { value: 'json', label: t('extend_tables.json_mode') },
          ]}
          disabledValues={table.value_fields.length === 0 ? ['form'] : []}
        />
        {mode === 'form' ? (
          <div className="overflow-hidden rounded-lg border border-bd-0">
            {table.value_fields.map((field) => (
              <div
                key={field.name}
                className="grid gap-2 border-b border-bd-0 bg-bg-1 p-4 last:border-b-0"
              >
                <div className="flex items-center justify-between gap-3">
                  <label
                    htmlFor={`extend-field-${field.name}`}
                    className="font-mono text-xs font-strong text-tx-0"
                  >
                    {field.name}
                    {field.required && <span className="ml-1 text-red">*</span>}
                  </label>
                  <span className="rounded bg-bg-3 px-1.5 py-0.5 font-mono text-type-micro uppercase text-tx-2">
                    {t(`extend_tables.types.${field.field_type}`)}
                  </span>
                </div>
                {field.description && (
                  <p className="text-xs text-tx-3">{field.description}</p>
                )}
                {field.field_type === 'boolean' ? (
                  <FormSelect
                    value={values[field.name] ?? ''}
                    onChange={(value) =>
                      setValues((current) => ({ ...current, [field.name]: value }))
                    }
                    placeholder={t('extend_tables.select_boolean')}
                    options={[
                      { value: '', label: t('extend_tables.not_set') },
                      { value: 'true', label: 'true' },
                      { value: 'false', label: 'false' },
                    ]}
                  />
                ) : (
                  <FormInput
                    id={`extend-field-${field.name}`}
                    className={field.field_type === 'object' ? 'font-mono text-xs' : ''}
                    value={values[field.name] ?? ''}
                    onChange={(event) =>
                      setValues((current) => ({
                        ...current,
                        [field.name]: event.target.value,
                      }))
                    }
                    placeholder={
                      field.field_type === 'object'
                        ? '{"key":"value"}'
                        : t('extend_tables.field_value_placeholder')
                    }
                  />
                )}
              </div>
            ))}
          </div>
        ) : (
          <FormTextarea
            className="min-h-72 font-mono text-xs"
            value={json}
            onChange={(event) => setJson(event.target.value)}
            aria-label={t('extend_tables.json_mode')}
          />
        )}
        {willOverwrite && (
          <InlineNotice tone="warning">
            {t('extend_tables.duplicate_key_warning')}
          </InlineNotice>
        )}
        {validationError && (
          <InlineNotice tone="warning">{validationError}</InlineNotice>
        )}
        {!validationError && key.trim() && (
          <InlineNotice tone="success">
            {editingRow || willOverwrite
              ? t('extend_tables.validation_overwrite')
              : t('extend_tables.validation_create')}
          </InlineNotice>
        )}
      </FormSection>
    </FormDrawer>
  );
}

export function ImportExtendRowsDrawer({
  access,
  open,
  table,
  rows,
  onClose,
}: {
  access: ActionAccess;
  open: boolean;
  table: extendTablesApi.ExtendTableSummary;
  rows: extendTablesApi.ExtendRow[];
  onClose: () => void;
}) {
  const { t } = useTranslation('functions');
  const qc = useQueryClient();
  const [text, setText] = React.useState('');
  const fileRef = React.useRef<HTMLInputElement>(null);

  React.useEffect(() => {
    if (open) setText('');
  }, [open]);

  const parsed = React.useMemo(() => {
    if (!text.trim()) return { records: [] as ImportRecord[], error: '' };
    try {
      return { records: parseImportText(text, table.key_field), error: '' };
    } catch {
      return { records: [] as ImportRecord[], error: t('extend_tables.import_invalid') };
    }
  }, [table.key_field, t, text]);
  const existingKeys = React.useMemo(
    () => new Set(rows.map((row) => row.key)),
    [rows],
  );
  const overwriteCount = parsed.records.filter((record) =>
    existingKeys.has(record.key),
  ).length;

  const importRows = useMutation({
    mutationFn: async () => {
      for (const record of parsed.records) {
        await extendTablesApi.upsert(table.table_name, record.key, record.value);
      }
    },
    onSuccess: async () => {
      await Promise.all([
        qc.invalidateQueries({
          queryKey: ['extend-tables', 'rows', table.table_name],
        }),
        qc.invalidateQueries({ queryKey: ['extend-tables', 'list'] }),
      ]);
      toast.success(
        t('extend_tables.toast_imported', { count: parsed.records.length }),
      );
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const readFile = async (file: File | undefined) => {
    if (!file) return;
    setText(await file.text());
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('extend_tables.import_title')}
      subtitle={t('extend_tables.import_subtitle', { table: table.table_name })}
      width={680}
      footer={
        <>
          <ChromeButton onClick={onClose}>{t('extend_tables.cancel')}</ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={
              access.disabled ||
              importRows.isPending ||
              parsed.records.length === 0 ||
              Boolean(parsed.error)
            }
            disabledReason={access.reason}
            onClick={() => access.allowed && importRows.mutate()}
          >
            <Upload className="h-3.5 w-3.5" />
            {importRows.isPending
              ? t('extend_tables.importing')
              : t('extend_tables.import_records', {
                  count: parsed.records.length,
                })}
          </ChromeButton>
        </>
      }
    >
      <fieldset
        disabled={access.disabled || importRows.isPending}
        aria-disabled={access.disabled || undefined}
        className="contents disabled:cursor-not-allowed"
      >
      <FormSection
        title={t('extend_tables.import_source')}
        description={t('extend_tables.import_source_description', {
          key: table.key_field,
        })}
      >
        <input
          ref={fileRef}
          type="file"
          accept=".json,.csv,application/json,text/csv"
          className="hidden"
          onChange={(event) => void readFile(event.target.files?.[0])}
        />
        <div className="grid gap-3 sm:grid-cols-2">
          <button
            type="button"
            onClick={() => fileRef.current?.click()}
            className="flex min-h-20 items-center gap-3 rounded-lg border border-bd-1 bg-bg-1 px-4 text-left hover:border-bd-2 hover:bg-bg-2"
          >
            <FileJson2 className="h-5 w-5 text-indigo-soft" />
            <span>
              <span className="block text-sm font-strong text-tx-0">
                JSON
              </span>
              <span className="mt-0.5 block text-xs text-tx-3">
                {t('extend_tables.json_file_description')}
              </span>
            </span>
          </button>
          <button
            type="button"
            onClick={() => fileRef.current?.click()}
            className="flex min-h-20 items-center gap-3 rounded-lg border border-bd-1 bg-bg-1 px-4 text-left hover:border-bd-2 hover:bg-bg-2"
          >
            <FileSpreadsheet className="h-5 w-5 text-green" />
            <span>
              <span className="block text-sm font-strong text-tx-0">CSV</span>
              <span className="mt-0.5 block text-xs text-tx-3">
                {t('extend_tables.csv_file_description')}
              </span>
            </span>
          </button>
        </div>
        <FormField label={t('extend_tables.paste_data')}>
          <FormTextarea
            className="min-h-72 font-mono text-xs"
            value={text}
            onChange={(event) => setText(event.target.value)}
            placeholder={`${table.key_field},tier,owner\ncustomer-1001,pro,platform`}
          />
        </FormField>
        {parsed.error && (
          <InlineNotice tone="warning">{parsed.error}</InlineNotice>
        )}
        {!parsed.error && parsed.records.length > 0 && (
          <InlineNotice tone={overwriteCount > 0 ? 'warning' : 'success'}>
            {overwriteCount > 0
              ? t('extend_tables.import_overwrite_summary', {
                  count: parsed.records.length,
                  overwrite: overwriteCount,
                })
              : t('extend_tables.import_ready', {
                  count: parsed.records.length,
                })}
          </InlineNotice>
        )}
      </FormSection>
      </fieldset>
    </FormDrawer>
  );
}

function StepIndicator({ step }: { step: 1 | 2 }) {
  const { t } = useTranslation('functions');
  return (
    <div className="grid grid-cols-[1fr_auto_1fr] items-center gap-3">
      <StepItem
        active={step === 1}
        complete={step > 1}
        number={1}
        label={t('extend_tables.step_definition')}
      />
      <div className="h-px w-10 bg-bd-1" />
      <StepItem
        active={step === 2}
        complete={false}
        number={2}
        label={t('extend_tables.step_data')}
      />
    </div>
  );
}

function StepItem({
  active,
  complete,
  number,
  label,
}: {
  active: boolean;
  complete: boolean;
  number: number;
  label: string;
}) {
  return (
    <div
      className={cn(
        'flex min-w-0 items-center gap-2 text-xs',
        active || complete ? 'text-tx-0' : 'text-tx-3',
      )}
    >
      <span
        className={cn(
          'grid h-6 w-6 shrink-0 place-items-center rounded-full border font-mono text-type-micro',
          active && 'border-indigo bg-indigo text-white',
          complete && 'border-green bg-green-dim text-green',
          !active && !complete && 'border-bd-1 bg-bg-2',
        )}
      >
        {complete ? <Check className="h-3.5 w-3.5" /> : number}
      </span>
      <span className="truncate font-strong">{label}</span>
    </div>
  );
}

function StartModeCard({
  active,
  icon: Icon,
  title,
  description,
  onClick,
}: {
  active: boolean;
  icon: LucideIcon;
  title: string;
  description: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'flex min-h-28 items-start gap-3 rounded-lg border p-4 text-left transition-colors',
        active
          ? 'border-indigo bg-indigo-dim'
          : 'border-bd-1 bg-bg-1 hover:border-bd-2 hover:bg-bg-2',
      )}
    >
      <Icon
        className={cn(
          'mt-0.5 h-5 w-5 shrink-0',
          active ? 'text-indigo-soft' : 'text-tx-3',
        )}
      />
      <span>
        <span className="block text-sm font-strong text-tx-0">{title}</span>
        <span className="mt-1 block text-xs leading-relaxed text-tx-2">
          {description}
        </span>
      </span>
    </button>
  );
}

function SegmentedMode<T extends string>({
  value,
  onChange,
  options,
  disabledValues = [],
}: {
  value: T;
  onChange: (value: T) => void;
  options: Array<{ value: T; label: string }>;
  disabledValues?: T[];
}) {
  return (
    <div className="inline-flex self-start rounded-md border border-bd-1 bg-bg-2 p-0.5">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          disabled={disabledValues.includes(option.value)}
          onClick={() => onChange(option.value)}
          className={cn(
            'h-7 rounded px-3 text-xs font-strong transition-colors',
            option.value === value
              ? 'bg-bg-1 text-tx-0 shadow-sm'
              : 'text-tx-2 hover:text-tx-0',
            'disabled:cursor-not-allowed disabled:opacity-40',
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

function InlineNotice({
  tone,
  children,
}: {
  tone: 'success' | 'warning';
  children: React.ReactNode;
}) {
  const Icon = tone === 'success' ? Check : AlertTriangle;
  return (
    <div
      className={cn(
        'flex items-start gap-2 rounded-md border px-3 py-2.5 text-xs leading-relaxed',
        tone === 'success'
          ? 'border-green/25 bg-green-dim text-green'
          : 'border-yellow/25 bg-yellow-dim text-yellow',
      )}
    >
      <Icon className="mt-0.5 h-3.5 w-3.5 shrink-0" />
      <span>{children}</span>
    </div>
  );
}
