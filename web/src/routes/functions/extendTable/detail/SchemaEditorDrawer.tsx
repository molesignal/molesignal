import { KeyRound, LockKeyhole, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type {
  ExtendFieldType,
  ExtendTableSummary,
  ExtendValueField,
} from '@/api/extendTables';
import type { ActionAccess } from '@/product/actionAccess';
import { ChromeButton, IconButton } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSelect,
} from '@/shell/FormDrawer';
import { Checkbox } from '@/shell/ui/checkbox';

const FIELD_NAME_PATTERN = /^[A-Za-z0-9_.-]+$/;
const MAX_FIELDS = 100;

interface DraftField {
  id: string;
  existing: boolean;
  value: ExtendValueField;
}

function normalizeFields(fields: DraftField[]): ExtendValueField[] {
  return fields.map(({ value }) => ({
    ...value,
    name: value.name.trim(),
    description: value.description.trim(),
  }));
}

function makeExistingDrafts(fields: ExtendValueField[]): DraftField[] {
  return fields.map((field, index) => ({
    id: `existing-${index}`,
    existing: true,
    value: { ...field },
  }));
}

export function SchemaEditorDrawer({
  access,
  open,
  table,
  fields,
  busy,
  onClose,
  onSave,
}: {
  access: ActionAccess;
  open: boolean;
  table: ExtendTableSummary;
  fields: ExtendValueField[];
  busy: boolean;
  onClose: () => void;
  onSave: (fields: ExtendValueField[]) => void;
}) {
  const { t } = useTranslation('functions');
  const [drafts, setDrafts] = React.useState<DraftField[]>(() =>
    makeExistingDrafts(fields),
  );
  const nextId = React.useRef(fields.length);

  React.useEffect(() => {
    if (!open) return;
    setDrafts(makeExistingDrafts(fields));
    nextId.current = fields.length;
  }, [fields, open, table.table_name]);

  const normalized = React.useMemo(() => normalizeFields(drafts), [drafts]);
  const original = React.useMemo(
    () => fields.map((field) => ({ ...field, description: field.description.trim() })),
    [fields],
  );
  const names = normalized.map((field) => field.name);
  const duplicateNames = new Set(names).size !== names.length;
  const emptyName = normalized.some((field) => !field.name);
  const invalidName = normalized.some(
    (field) => field.name && !FIELD_NAME_PATTERN.test(field.name),
  );
  const changed = JSON.stringify(normalized) !== JSON.stringify(original);
  const formDisabled = busy || access.disabled;

  let validationError: string | undefined;
  if (emptyName) {
    validationError = t('extend_tables.field_name_required');
  } else if (invalidName) {
    validationError = t('extend_tables.field_name_invalid');
  } else if (duplicateNames) {
    validationError = t('extend_tables.duplicate_fields');
  }

  const saveDisabled =
    access.disabled || busy || Boolean(validationError) || !changed;
  const saveDisabledReason = access.reason
    ?? validationError
    ?? (!changed ? t('extend_tables.no_schema_changes') : undefined);

  const updateField = (
    id: string,
    patch: Partial<ExtendValueField>,
  ) => {
    setDrafts((current) =>
      current.map((draft) =>
        draft.id === id
          ? { ...draft, value: { ...draft.value, ...patch } }
          : draft,
      ),
    );
  };

  const addField = () => {
    const id = `new-${nextId.current}`;
    nextId.current += 1;
    setDrafts((current) => [
      ...current,
      {
        id,
        existing: false,
        value: {
          name: '',
          field_type: 'string',
          required: false,
          description: '',
        },
      },
    ]);
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(nextOpen) => !nextOpen && !busy && onClose()}
      title={t('extend_tables.schema_edit_title')}
      subtitle={t('extend_tables.schema_edit_subtitle')}
      width={900}
      headerDivider={false}
      bodyClassName="px-0 py-0"
      footer={
        <>
          <ChromeButton disabled={busy} onClick={onClose}>
            {t('extend_tables.cancel')}
          </ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={saveDisabled}
            disabledReason={saveDisabledReason}
            onClick={() => access.allowed && onSave(normalized)}
          >
            {busy
              ? t('extend_tables.saving')
              : t('extend_tables.save_changes')}
          </ChromeButton>
        </>
      }
    >
      <dl className="grid gap-4 border-b border-bd-0 px-6 py-5 sm:grid-cols-2">
        <div>
          <dt className="flex items-center gap-1.5 text-type-micro uppercase tracking-wider text-tx-3">
            <LockKeyhole className="h-3 w-3" />
            {t('extend_tables.columns.name')}
          </dt>
          <dd className="mt-1.5 font-mono text-xs font-strong text-tx-0">
            {table.table_name}
          </dd>
        </div>
        <div>
          <dt className="flex items-center gap-1.5 text-type-micro uppercase tracking-wider text-tx-3">
            <KeyRound className="h-3 w-3" />
            {t('extend_tables.key_field')}
          </dt>
          <dd className="mt-1.5 font-mono text-xs font-strong text-tx-0">
            {table.key_field}
          </dd>
        </div>
      </dl>

      <div className="px-6 py-5">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 className="text-sm font-display-strong text-tx-0">
              {t('extend_tables.value_fields')}
            </h2>
            <p className="mt-1 max-w-2xl text-xs leading-relaxed text-tx-2">
              {table.row_count > 0
                ? t('extend_tables.schema_locked_with_rows', {
                    count: table.row_count,
                  })
                : t('extend_tables.schema_destructive_allowed_empty')}
            </p>
          </div>
          <ChromeButton
            size="sm"
            disabled={formDisabled || drafts.length >= MAX_FIELDS}
            disabledReason={
              access.reason
              ?? (drafts.length >= MAX_FIELDS
                ? t('extend_tables.field_limit', { count: MAX_FIELDS })
                : undefined)
            }
            onClick={addField}
          >
            <Plus className="h-3.5 w-3.5" />
            {t('extend_tables.add_value_field')}
          </ChromeButton>
        </div>

        {validationError && (
          <p
            role="alert"
            className="mt-4 border-l-2 border-yellow px-3 py-2 text-xs leading-relaxed text-yellow-soft"
          >
            {validationError}
          </p>
        )}

        {drafts.length === 0 ? (
          <p className="mt-5 border-y border-bd-0 py-6 text-center text-xs text-tx-3">
            {t('extend_tables.schema_no_value_fields')}
          </p>
        ) : (
          <div className="mt-5 divide-y divide-bd-0 border-y border-bd-0">
            {drafts.map((draft, index) => {
              const locked = table.row_count > 0 && draft.existing;
              const fieldLabel = draft.value.name
                || t('extend_tables.schema_field_number', {
                  number: index + 1,
                });
              return (
                <div key={draft.id} className="py-4">
                  <div className="mb-3 flex min-h-8 items-center justify-between gap-3">
                    <span className="font-mono text-type-micro uppercase tracking-wider text-tx-3">
                      {t('extend_tables.schema_field_number', {
                        number: index + 1,
                      })}
                    </span>
                    <IconButton
                      disabled={formDisabled || locked}
                      disabledReason={
                        access.reason
                        ?? (locked
                          ? t('extend_tables.schema_remove_locked')
                          : undefined)
                      }
                      aria-label={t('extend_tables.remove_field_aria', {
                        field: fieldLabel,
                      })}
                      className="h-11 w-11 enabled:hover:bg-red-dim enabled:hover:text-red sm:h-8 sm:w-8"
                      onClick={() =>
                        setDrafts((current) =>
                          current.filter((item) => item.id !== draft.id),
                        )
                      }
                    >
                      <Trash2 className="h-4 w-4" />
                    </IconButton>
                  </div>
                  <div className="grid gap-4 md:grid-cols-[minmax(0,1fr)_12rem]">
                    <FormField label={t('extend_tables.field_name')} required>
                      <FormInput
                        value={draft.value.name}
                        disabled={formDisabled || locked}
                        disabledReason={
                          access.reason
                          ?? (locked
                            ? t('extend_tables.schema_name_locked')
                            : undefined)
                        }
                        aria-label={t('extend_tables.field_name_aria', {
                          number: index + 1,
                        })}
                        className="h-11 text-base sm:h-9 sm:text-sm"
                        placeholder={t('extend_tables.field_name_placeholder')}
                        onChange={(event) =>
                          updateField(draft.id, { name: event.target.value })
                        }
                      />
                    </FormField>
                    <FormField label={t('extend_tables.field_type')} required>
                      <FormSelect
                        value={draft.value.field_type}
                        disabled={formDisabled || locked}
                        disabledReason={
                          access.reason
                          ?? (locked
                            ? t('extend_tables.schema_type_locked')
                            : undefined)
                        }
                        ariaLabel={t('extend_tables.field_type_aria', {
                          number: index + 1,
                        })}
                        className="h-11 text-base sm:h-9 sm:text-sm"
                        options={(
                          ['string', 'number', 'boolean', 'object'] as const
                        ).map((fieldType) => ({
                          value: fieldType,
                          label: t(`extend_tables.types.${fieldType}`),
                        }))}
                        onChange={(fieldType) =>
                          updateField(draft.id, {
                            field_type: fieldType as ExtendFieldType,
                          })
                        }
                      />
                    </FormField>
                  </div>
                  <div className="mt-4 grid items-end gap-4 md:grid-cols-[minmax(0,1fr)_12rem]">
                    <FormField label={t('extend_tables.field_description')}>
                      <FormInput
                        value={draft.value.description}
                        disabled={formDisabled}
                        disabledReason={access.reason}
                        maxLength={500}
                        aria-label={t('extend_tables.field_description_aria', {
                          number: index + 1,
                        })}
                        className="h-11 text-base sm:h-9 sm:text-sm"
                        placeholder={t(
                          'extend_tables.field_description_placeholder',
                        )}
                        onChange={(event) =>
                          updateField(draft.id, {
                            description: event.target.value,
                          })
                        }
                      />
                    </FormField>
                    <label className="flex min-h-11 items-center gap-2 text-sm text-tx-1">
                      <Checkbox
                        checked={draft.value.required}
                        disabled={formDisabled}
                        aria-label={t('extend_tables.field_required_aria', {
                          field: fieldLabel,
                        })}
                        className="h-5 w-5"
                        onCheckedChange={(checked) =>
                          updateField(draft.id, {
                            required: checked === true,
                          })
                        }
                      />
                      {t('extend_tables.required')}
                    </label>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </FormDrawer>
  );
}
