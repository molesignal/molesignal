import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable, PageHeader } from '@/admin';
import * as patternsApi from '@/api/regexPatterns';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import { SectionBody } from './_atoms';
import { formatMicros } from '../rum/_helpers';

export function RegexPatterns() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({
    permission: 'org.settings.manage',
  });
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<patternsApi.RegexPattern | null>(null);
  const [removing, setRemoving] = React.useState<patternsApi.RegexPattern | null>(null);

  const q = useQuery({
    queryKey: ['regex-patterns'],
    queryFn: () => patternsApi.list(),
  });
  const rows = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('regex_patterns.empty_title'),
    emptyDescription: t('regex_patterns.empty_description'),
  });

  const remove = useMutation({
    mutationFn: (id: string) => patternsApi.remove(id),
    onSuccess: () => {
      toast.success(tc('status.deleted'));
      void qc.invalidateQueries({ queryKey: ['regex-patterns'] });
      setRemoving(null);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const drawerOpen = creating || editing !== null;

  return (
    <>
      <PageHeader
        title={t('regex_patterns.title')}
        subtitle={t('regex_patterns.subtitle') as string}
        actions={
          <ChromeButton
            variant="primary"
            onClick={() => setCreating(true)}
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
          >
            {t('regex_patterns.new_pattern')}
          </ChromeButton>
        }
      />
      <PatternDrawer
        open={drawerOpen}
        editing={editing}
        access={manageAccess}
        onClose={() => {
          setCreating(false);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(v) => !v && setRemoving(null)}
        destructive
        title={t('regex_patterns.delete_title')}
        description={removing?.name ?? ''}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (removing && manageAccess.allowed) {
            remove.mutate(removing.id);
          }
        }}
      />
      <SectionBody>
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <DataTable
            rows={rows}
            rowKey={(r) => r.id}
            onRowClick={(r) => setEditing(r)}
            isRowClickDisabled={() => manageAccess.disabled}
            rowClickDisabledReason={() => manageAccess.reason}
            columns={[
              { key: 'name', header: t('regex_patterns.columns.name'), cell: (r) => r.name },
              {
                key: 'pattern',
                header: t('regex_patterns.columns.pattern'),
                cell: (r) => (
                  <code className="font-sans text-xs text-tx-1">{r.pattern}</code>
                ),
              },
              {
                key: 'replacement',
                header: t('regex_patterns.columns.replacement'),
                cell: (r) => (
                  <code className="font-sans text-xs text-tx-1">{r.replacement}</code>
                ),
              },
              {
                key: 'scope',
                header: t('regex_patterns.columns.scope'),
                width: 130,
                cell: (r) => (
                  <span className={r.apply_on_ingest ? 'text-tx-1' : 'text-tx-3'}>
                    {r.apply_on_ingest
                      ? t('regex_patterns.scope_ingest')
                      : t('regex_patterns.scope_query')}
                  </span>
                ),
              },
              {
                key: 'updated',
                header: t('regex_patterns.columns.updated'),
                cell: (r) => formatMicros(r.updated_at_micros),
                width: 180,
              },
              {
                key: 'actions',
                header: '',
                width: 130,
                cell: (r) => (
                  <div
                    className="flex justify-end gap-1"
                    onClick={(event) => event.stopPropagation()}
                  >
                    <ChromeButton
                      variant="ghost"
                      size="sm"
                      disabled={manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                      onClick={(e) => {
                        e.stopPropagation();
                        setEditing(r);
                      }}
                    >
                      {tc('actions.edit')}
                    </ChromeButton>
                    <ChromeButton
                      variant="ghost"
                      size="sm"
                      disabled={manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                      onClick={(e) => {
                        e.stopPropagation();
                        setRemoving(r);
                      }}
                      className="enabled:hover:text-red-soft"
                    >
                      {tc('actions.delete')}
                    </ChromeButton>
                  </div>
                ),
              },
            ]}
          />
        )}
      </SectionBody>
    </>
  );
}

function PatternDrawer({
  open,
  editing,
  access,
  onClose,
}: {
  open: boolean;
  editing: patternsApi.RegexPattern | null;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const [name, setName] = React.useState('');
  const [pattern, setPattern] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [replacement, setReplacement] = React.useState('');
  const [applyOnIngest, setApplyOnIngest] = React.useState(false);

  React.useEffect(() => {
    if (open) {
      setName(editing?.name ?? '');
      setPattern(editing?.pattern ?? '');
      setDescription(editing?.description ?? '');
      setReplacement(editing?.replacement ?? '[REDACTED]');
      setApplyOnIngest(editing?.apply_on_ingest ?? false);
    }
  }, [open, editing]);

  const save = useMutation({
    mutationFn: () => {
      const input: patternsApi.CreateRegexPatternInput = {
        name,
        pattern,
        description,
        replacement,
        apply_on_ingest: applyOnIngest,
      };
      return editing ? patternsApi.update(editing.id, input) : patternsApi.create(input);
    },
    onSuccess: () => {
      toast.success(editing ? tc('status.updated') : tc('status.created'));
      void qc.invalidateQueries({ queryKey: ['regex-patterns'] });
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });
  const invalid = name.trim().length === 0 || pattern.trim().length === 0;
  const dirty =
    !editing ||
    name !== editing.name ||
    pattern !== editing.pattern ||
    description !== editing.description ||
    replacement !== editing.replacement ||
    applyOnIngest !== editing.apply_on_ingest;
  const submitDisabled = access.disabled || !dirty;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(v) => !v && onClose()}
      title={editing ? t('regex_patterns.edit_title') : t('regex_patterns.drawer_title')}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={submitDisabled}
          invalid={invalid}
          disabledReason={
            access.reason ??
            (invalid
              ? tc('access.form_invalid')
              : !dirty
                ? tc('access.no_changes')
                : undefined)
          }
          onCancel={onClose}
          submitLabel={editing ? tc('actions.save') : t('regex_patterns.submit_label')}
          formId="regex-pattern-form"
        />
      }
    >
      <form
        id="regex-pattern-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (submitDisabled || invalid || save.isPending) return;
          save.mutate();
        }}
      >
        <FormSection>
          <FormField label={t('regex_patterns.field_name')} required>
            <FormInput
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={access.disabled || save.isPending}
              disabledReason={access.reason}
              required
            />
          </FormField>
          <FormField
            label={t('regex_patterns.field_pattern')}
            required
            hint={t('regex_patterns.field_pattern_hint')}
          >
            <FormTextarea
              value={pattern}
              onChange={(e) => setPattern(e.target.value)}
              rows={4}
              className="font-sans text-xs"
              disabled={access.disabled || save.isPending}
              disabledReason={access.reason}
              required
            />
          </FormField>
          <FormField
            label={t('regex_patterns.field_replacement')}
            hint={t('regex_patterns.field_replacement_hint')}
          >
            <FormInput
              value={replacement}
              onChange={(e) => setReplacement(e.target.value)}
              className="font-sans text-xs"
              disabled={access.disabled || save.isPending}
              disabledReason={access.reason}
            />
          </FormField>
          <FormField
            label={t('regex_patterns.field_apply_on_ingest')}
            hint={t('regex_patterns.field_apply_on_ingest_hint')}
          >
            <Switch
              checked={applyOnIngest}
              onCheckedChange={setApplyOnIngest}
              disabled={access.disabled || save.isPending}
              disabledReason={access.reason}
            />
          </FormField>
          <FormField label={t('regex_patterns.field_description')}>
            <FormInput
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              disabled={access.disabled || save.isPending}
              disabledReason={access.reason}
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}
