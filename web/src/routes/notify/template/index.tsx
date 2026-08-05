import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import type { NotifyCategory } from '@/api/notify';
import * as templatesApi from '@/api/notify/templates';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSelect,
  FormTextarea,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { formatMicros, NOTIFY_CATEGORIES } from '../model';
import { NotifySettingsPage } from '../SettingsPage';
import { TemplateMessagePreview } from './MessagePreview';
import {
  defaultNotifyTemplatePreview,
  renderNotifyTemplate,
} from './model';
import {
  TemplateReferencePanel,
  useTemplateTokenInserter,
} from './ReferencePanel';

type TemplateFormat = NonNullable<templatesApi.NotifyTemplate['format']>;

export function NotifyTemplatesPage() {
  const { t, i18n } = useTranslation('notify');
  const qc = useQueryClient();
  const manage = useActionAccess({ permission: 'alerts.manage' });
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] =
    React.useState<templatesApi.NotifyTemplate | null>(null);
  const [removing, setRemoving] =
    React.useState<templatesApi.NotifyTemplate | null>(null);
  const templates = useQuery({
    queryKey: ['notify', 'templates'],
    queryFn: templatesApi.list,
  });
  const rows = templates.data ?? [];
  const state = productStateFor(
    queryStateFor({
      isLoading: templates.isLoading,
      isError: templates.isError,
      data: rows,
    }),
    {
      error: templates.error,
      emptyTitle: t('templates.empty_title'),
      emptyDescription: t('templates.empty_description'),
    },
  );
  const remove = useMutation({
    mutationFn: templatesApi.remove,
    onSuccess: () => {
      toast.success(t('common.deleted'));
      setRemoving(null);
      void qc.invalidateQueries({ queryKey: ['notify', 'templates'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <>
      <NotifySettingsPage
        title={t('templates.title')}
        subtitle={t('templates.subtitle')}
        toolbar={
          <ChromeButton
            variant="primary"
            disabled={manage.disabled}
            disabledReason={manage.reason}
            onClick={() => setCreating(true)}
          >
            <Plus className="h-4 w-4" />
            {t('templates.new')}
          </ChromeButton>
        }
        state={state}
      >
        <div className="overflow-x-auto rounded-md border border-bd-0 bg-bg-1">
          <DataTable
            rows={rows}
            rowKey={(row) => row.id}
            onRowClick={(row) => manage.allowed && setEditing(row)}
            isRowClickDisabled={() => manage.disabled}
            rowClickDisabledReason={() => manage.reason}
            columns={[
              {
                key: 'name',
                header: t('templates.columns.name'),
                cell: (row) => (
                  <span className="text-sm font-semibold text-tx-0">
                    {row.name}
                  </span>
                ),
              },
              {
                key: 'format',
                header: t('templates.columns.format'),
                width: 140,
                cell: (row) => (
                  <Pill tone="dim">{row.format ?? 'text'}</Pill>
                ),
              },
              {
                key: 'category',
                header: t('templates.columns.category'),
                width: 140,
                cell: (row) => (
                  <Pill tone={row.category === 'oncall' ? 'orange' : 'dim'}>
                    {t(`preferences.${row.category}`)}
                  </Pill>
                ),
              },
              {
                key: 'updated',
                header: t('templates.columns.updated'),
                width: 200,
                cell: (row) => (
                  <span className="text-xs text-tx-2">
                    {row.updated_at_micros
                      ? formatMicros(row.updated_at_micros, i18n.language)
                      : '—'}
                  </span>
                ),
              },
              {
                key: 'actions',
                header: t('templates.columns.actions'),
                width: 120,
                className: 'text-right',
                headerClassName: 'text-right',
                cell: (row) => (
                  <ChromeButton
                    size="sm"
                    disabled={manage.disabled}
                    disabledReason={manage.reason}
                    onClick={(event) => {
                      event.stopPropagation();
                      setRemoving(row);
                    }}
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                    {t('common.delete')}
                  </ChromeButton>
                ),
              },
            ]}
          />
        </div>
      </NotifySettingsPage>
      <TemplateEditor
        open={creating || editing !== null}
        template={editing}
        onClose={() => {
          setCreating(false);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('templates.delete_title')}
        description={t('templates.delete_description', {
          name: removing?.name ?? '',
        })}
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        busy={remove.isPending}
        onConfirm={() => removing && remove.mutate(removing.id)}
      />
    </>
  );
}

function TemplateEditor({
  open,
  template,
  onClose,
}: {
  open: boolean;
  template: templatesApi.NotifyTemplate | null;
  onClose: () => void;
}) {
  const { t } = useTranslation('notify');
  const qc = useQueryClient();
  const [name, setName] = React.useState('');
  const [category, setCategory] =
    React.useState<NotifyCategory>('alert');
  const [format, setFormat] = React.useState<TemplateFormat>('text');
  const [body, setBody] = React.useState('');
  const bodyRef = React.useRef<HTMLTextAreaElement>(null);
  const insertToken = useTemplateTokenInserter(bodyRef, setBody);
  const [attributes, setAttributes] = React.useState(
    JSON.stringify(
      defaultNotifyTemplatePreview('alert').attributes,
      null,
      2,
    ),
  );
  const preview = React.useMemo(() => {
    try {
      const parsed: unknown = JSON.parse(attributes);
      if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') {
        throw new Error(t('templates.attributes_object'));
      }
      const input = defaultNotifyTemplatePreview(category);
      return {
        rendered: renderNotifyTemplate(body, {
          ...input,
          attributes: parsed as Record<string, unknown>,
        }),
        error: null,
      };
    } catch (error) {
      return {
        rendered: '',
        error: error instanceof Error ? error.message : String(error),
      };
    }
  }, [attributes, body, category, t]);
  React.useEffect(() => {
    if (!open) return;
    const nextCategory = template?.category ?? 'alert';
    setName(template?.name ?? '');
    setCategory(nextCategory);
    setFormat(template?.format ?? 'text');
    setBody(template?.body ?? '');
    setAttributes(
      JSON.stringify(
        defaultNotifyTemplatePreview(nextCategory).attributes,
        null,
        2,
      ),
    );
  }, [open, template]);
  const save = useMutation({
    mutationFn: () => {
      const input = { name: name.trim(), body, format, category };
      return template
        ? templatesApi.update(template.id, input)
        : templatesApi.create(input);
    },
    onSuccess: () => {
      toast.success(t('common.saved'));
      void qc.invalidateQueries({ queryKey: ['notify', 'templates'] });
      onClose();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const changeCategory = (nextCategory: NotifyCategory) => {
    setCategory(nextCategory);
    setAttributes(
      JSON.stringify(
        defaultNotifyTemplatePreview(nextCategory).attributes,
        null,
        2,
      ),
    );
  };
  const usePreset = (preset: templatesApi.NotifyTemplatePreset) => {
    changeCategory(preset.category);
    setFormat(preset.format);
    setBody(preset.body);
    window.requestAnimationFrame(() => {
      bodyRef.current?.focus();
      bodyRef.current?.setSelectionRange(
        preset.body.length,
        preset.body.length,
      );
    });
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      width={1180}
      title={
        template
          ? t('templates.edit_title', { name: template.name })
          : t('templates.new_title')
      }
      subtitle={t('templates.editor_subtitle')}
      footer={
        <>
          <ChromeButton onClick={onClose}>{t('common.cancel')}</ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={
              !name.trim() ||
              !body.trim() ||
              preview.error !== null ||
              save.isPending
            }
            onClick={() => save.mutate()}
          >
            {save.isPending ? t('common.saving') : t('common.save')}
          </ChromeButton>
        </>
      }
    >
      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_minmax(390px,1fr)]">
        <div className="space-y-4">
          <FormField label={t('templates.name')} required>
            <FormInput
              value={name}
              onChange={(event) => setName(event.target.value)}
            />
          </FormField>
          <div className="grid gap-4 sm:grid-cols-2">
            <FormField label={t('templates.category')}>
              <FormSelect
                value={category}
                onChange={(value) =>
                  changeCategory(value as NotifyCategory)
                }
                options={NOTIFY_CATEGORIES.map((value) => ({
                  value,
                  label: t(`preferences.${value}`),
                }))}
              />
            </FormField>
            <FormField label={t('templates.format')}>
              <FormSelect
                value={format}
                onChange={(value) => setFormat(value as TemplateFormat)}
                options={['text', 'markdown', 'html']}
              />
            </FormField>
          </div>
          <FormField
            label={t('templates.body')}
            hint={t('templates.variables_hint')}
            required
          >
            <FormTextarea
              ref={bodyRef}
              className="min-h-72 font-mono text-xs"
              value={body}
              onChange={(event) => setBody(event.target.value)}
            />
          </FormField>
        </div>
        <div className="space-y-4">
          <TemplateReferencePanel
            key={category}
            category={category}
            onInsert={insertToken}
            onUsePreset={usePreset}
          />
          <FormField label={t('templates.preview_attributes')}>
            <FormTextarea
              className="min-h-48 font-mono text-xs"
              value={attributes}
              onChange={(event) => setAttributes(event.target.value)}
            />
          </FormField>
          <section className="rounded-md border border-bd-0 bg-bg-2 p-4">
            <h3 className="text-xs font-semibold uppercase tracking-wide text-tx-2">
              {t('templates.preview')}
            </h3>
            {preview.error ? (
              <p className="mt-3 text-xs text-red-soft">{preview.error}</p>
            ) : (
              <TemplateMessagePreview
                format={format}
                content={preview.rendered}
                emptyText={t('templates.preview_empty')}
              />
            )}
          </section>
        </div>
      </div>
    </FormDrawer>
  );
}
