import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as templatesApi from '@/api/notify/templates';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { toast } from '@/shell/ui/sonner';

import { formatMicros } from '../model';
import { NotifySettingsPage } from '../SettingsPage';
import { TEMPLATE_CHANNELS, type TemplateChannel, templateChannelLabel } from './model';
import { TemplateEditor } from './TemplateEditor';

function channelOfTemplate(template: templatesApi.NotifyTemplate): TemplateChannel {
  return template.channel ?? 'general';
}

export function NotifyTemplatesPage() {
  const { t, i18n } = useTranslation('notify');
  const qc = useQueryClient();
  const manage = useActionAccess({ permission: 'notify.templates.manage' });
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] =
    React.useState<templatesApi.NotifyTemplate | null>(null);
  const [removing, setRemoving] =
    React.useState<templatesApi.NotifyTemplate | null>(null);
  const templates = useQuery({
    queryKey: ['notify', 'templates'],
    queryFn: templatesApi.list,
  });
  const [shown, setShown] = React.useState<'all' | TemplateChannel>('all');
  const all = React.useMemo(
    () =>
      [...(templates.data ?? [])].sort(
        (left, right) =>
          TEMPLATE_CHANNELS.indexOf(channelOfTemplate(left)) -
            TEMPLATE_CHANNELS.indexOf(channelOfTemplate(right)) ||
          left.name.localeCompare(right.name),
      ),
    [templates.data],
  );
  const present = TEMPLATE_CHANNELS.filter((channel) =>
    all.some((template) => channelOfTemplate(template) === channel),
  );
  // A channel whose last template is gone shows them all again.
  const channel = shown !== 'all' && present.includes(shown) ? shown : 'all';
  const rows =
    channel === 'all'
      ? all
      : all.filter((template) => channelOfTemplate(template) === channel);
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
        filters={
          present.length > 1 ? (
            <ViewTabs<'all' | TemplateChannel>
              label={t('templates.channel_filter')}
              value={channel}
              onValueChange={setShown}
              items={[
                { key: 'all', label: t('templates.all'), count: all.length },
                ...present.map((key) => ({
                  key,
                  label: templateChannelLabel(t, key),
                  count: all.filter((template) => channelOfTemplate(template) === key)
                    .length,
                })),
              ]}
            />
          ) : undefined
        }
      >
        <div className="overflow-x-auto">
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
                key: 'channel',
                header: t('templates.columns.channel'),
                width: 140,
                cell: (row) => (
                  <span className="text-sm text-tx-1">
                    {templateChannelLabel(t, channelOfTemplate(row))}
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
                    {row.updatedAt
                      ? formatMicros(row.updatedAt, i18n.language)
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
