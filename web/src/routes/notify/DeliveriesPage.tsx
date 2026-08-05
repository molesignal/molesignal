import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, RotateCcw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as notifyApi from '@/api/notify';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import { FormDrawer, FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import {
  connectorName,
  deliveryStages,
  formatMicros,
  statusTone,
} from './model';
import { NotifySettingsPage } from './SettingsPage';

const STATUSES: notifyApi.NotifyDeliveryStatus[] = [
  'pending',
  'sending',
  'success',
  'failed',
  'skipped',
  'acknowledged',
];

export function NotifyDeliveriesPage() {
  const { t, i18n } = useTranslation('notify');
  const qc = useQueryClient();
  const acknowledge = useActionAccess({ permission: 'alerts.acknowledge' });
  const manage = useActionAccess({ permission: 'alerts.manage' });
  const [eventId, setEventId] = React.useState('');
  const [status, setStatus] = React.useState('');
  const [stage, setStage] = React.useState('');
  const [selected, setSelected] =
    React.useState<notifyApi.NotifyDelivery | null>(null);
  const deliveries = useQuery({
    queryKey: ['notify', 'deliveries', eventId, status, stage],
    queryFn: () =>
      notifyApi.listDeliveries({
        event_id: eventId.trim() || undefined,
        status: status || undefined,
        stage: stage || undefined,
        limit: 200,
      }),
  });
  const connectors = useQuery({
    queryKey: ['notify', 'connectors'],
    queryFn: notifyApi.listConnectors,
  });
  const policies = useQuery({
    queryKey: ['notify', 'policies'],
    queryFn: notifyApi.listPolicies,
  });
  const chain = useQuery({
    queryKey: ['notify', 'deliveries', 'event', selected?.event_id],
    queryFn: () =>
      notifyApi.listDeliveries({
        event_id: selected?.event_id ?? '',
        limit: 500,
      }),
    enabled: selected !== null,
  });
  const rows = deliveries.data ?? [];
  const state = productStateFor(
    queryStateFor({
      isLoading: deliveries.isLoading,
      isError: deliveries.isError,
      data: rows,
    }),
    {
      error: deliveries.error,
      emptyTitle: t('deliveries.empty_title'),
      emptyDescription: t('deliveries.empty_description'),
    },
  );
  const ack = useMutation({
    mutationFn: (id: string) => notifyApi.acknowledgeDelivery(id),
    onSuccess: () => {
      toast.success(t('deliveries.acknowledged'));
      void qc.invalidateQueries({ queryKey: ['notify', 'deliveries'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const retry = useMutation({
    mutationFn: (id: string) => notifyApi.retryDelivery(id),
    onSuccess: () => {
      toast.success(t('deliveries.retried'));
      void qc.invalidateQueries({ queryKey: ['notify', 'deliveries'] });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const eventChain = (chain.data ?? [])
    .slice()
    .sort((left, right) =>
      left.created_at === right.created_at
        ? left.attempt - right.attempt
        : left.created_at - right.created_at,
    );

  return (
    <>
      <NotifySettingsPage
        title={t('deliveries.title')}
        subtitle={t('deliveries.subtitle')}
        filters={
          <div className="grid w-full gap-3 md:grid-cols-3">
            <FormField label={t('deliveries.filters.event')}>
              <FormInput
                value={eventId}
                onChange={(event) => setEventId(event.target.value)}
              />
            </FormField>
            <FormField label={t('deliveries.filters.status')}>
              <FormSelect
                value={status}
                onChange={setStatus}
                options={[
                  { value: '', label: t('deliveries.filters.all') },
                  ...STATUSES.map((value) => ({
                    value,
                    label: t(`statuses.${value}`),
                  })),
                ]}
              />
            </FormField>
            <FormField label={t('deliveries.filters.stage')}>
              <FormSelect
                value={stage}
                onChange={setStage}
                options={[
                  { value: '', label: t('deliveries.filters.all') },
                  ...deliveryStages().map((value) => ({
                    value,
                    label: t(`stages.${value}`),
                  })),
                ]}
              />
            </FormField>
          </div>
        }
        state={state}
      >
        <div className="overflow-x-auto rounded-md border border-bd-0 bg-bg-1">
          <DataTable
            rows={rows}
            rowKey={(row) => row.id}
            onRowClick={setSelected}
            columns={[
              {
                key: 'event',
                header: t('deliveries.columns.event'),
                width: '24%',
                cell: (row) => (
                  <div>
                    <div className="truncate font-mono text-xs text-tx-1">{row.event_id}</div>
                    <div className="text-xs text-tx-3">#{row.attempt}</div>
                  </div>
                ),
              },
              {
                key: 'recipient',
                header: t('deliveries.columns.recipient'),
                width: '14%',
                cell: (row) => (
                  <span className="font-mono text-xs text-tx-2">
                    {row.recipient_user_id ?? '—'}
                  </span>
                ),
              },
              {
                key: 'source',
                header: t('deliveries.columns.source'),
                width: '14%',
                cell: (row) => (
                  <span className="text-xs text-tx-2">
                    {row.stage === 'test'
                      ? t('stages.test')
                      : (() => {
                          const resolver = (
                            policies.data ?? []
                          ).find(
                            (policy) => policy.id === row.policy_id,
                          )?.recipient_resolver;
                          return resolver
                            ? t(`resolver_types.${resolver}`, {
                                defaultValue: resolver,
                              })
                            : '—';
                        })()}
                  </span>
                ),
              },
              {
                key: 'connector',
                header: t('deliveries.columns.connector'),
                width: '13%',
                cell: (row) => (
                  <span className="text-xs text-tx-2">
                    {connectorName(connectors.data ?? [], row.connector_id)}
                  </span>
                ),
              },
              {
                key: 'stage',
                header: t('deliveries.columns.stage'),
                width: 150,
                cell: (row) => <Pill tone="dim">{t(`stages.${row.stage}`)}</Pill>,
              },
              {
                key: 'status',
                header: t('deliveries.columns.status'),
                width: 110,
                cell: (row) => (
                  <Pill tone={statusTone(row.status)}>{t(`statuses.${row.status}`)}</Pill>
                ),
              },
              {
                key: 'latency',
                header: t('deliveries.columns.latency'),
                width: 90,
                cell: (row) => (
                  <span className="font-mono text-xs text-tx-2">
                    {row.latency_ms === null || row.latency_ms === undefined
                      ? '—'
                      : `${row.latency_ms} ms`}
                  </span>
                ),
              },
              {
                key: 'sent',
                header: t('deliveries.columns.sent'),
                width: 170,
                cell: (row) => (
                  <span className="text-xs text-tx-2">
                    {formatMicros(row.sent_at ?? row.created_at, i18n.language)}
                  </span>
                ),
              },
            ]}
          />
        </div>
      </NotifySettingsPage>
      <FormDrawer
        open={selected !== null}
        onOpenChange={(next) => !next && setSelected(null)}
        width={720}
        title={t('deliveries.detail_title')}
        subtitle={
          selected
            ? t('deliveries.detail_subtitle', {
                attempt: selected.attempt,
                status: t(`statuses.${selected.status}`),
              })
            : undefined
        }
        footer={
          selected ? (
            <>
              <ChromeButton
                disabled={
                  acknowledge.disabled ||
                  ack.isPending ||
                  selected.status === 'acknowledged' ||
                  !selected.policy_id
                }
                disabledReason={acknowledge.reason}
                onClick={() => ack.mutate(selected.id)}
              >
                <Check className="h-4 w-4" />
                {t('deliveries.ack')}
              </ChromeButton>
              <ChromeButton
                variant="primary"
                disabled={
                  manage.disabled ||
                  retry.isPending ||
                  selected.status !== 'failed' ||
                  !selected.policy_id
                }
                disabledReason={manage.reason}
                onClick={() => retry.mutate(selected.id)}
              >
                <RotateCcw className="h-4 w-4" />
                {t('deliveries.retry')}
              </ChromeButton>
            </>
          ) : null
        }
      >
        <div className="space-y-3">
          {chain.isLoading && (
            <p className="text-xs text-tx-3">{t('common.loading')}</p>
          )}
          {eventChain.map((delivery, index) => (
            <div key={delivery.id} className="relative pl-7">
              {index < eventChain.length - 1 && (
                <span className="absolute left-[9px] top-5 h-[calc(100%+4px)] w-px bg-bd-1" />
              )}
              <span
                className={`absolute left-1 top-3 h-3 w-3 rounded-full ${
                  delivery.status === 'failed'
                    ? 'bg-red'
                    : delivery.status === 'acknowledged'
                      ? 'bg-green'
                      : 'bg-indigo'
                }`}
              />
              <div className="rounded-md border border-bd-0 bg-bg-2 p-3">
                <div className="flex flex-wrap items-center gap-2">
                  <Pill tone={statusTone(delivery.status)}>
                    {t(`statuses.${delivery.status}`)}
                  </Pill>
                  <Pill tone="dim">{t(`stages.${delivery.stage}`)}</Pill>
                  <span className="ml-auto font-mono text-xs text-tx-3">
                    #{delivery.attempt}
                  </span>
                </div>
                <div className="mt-2 text-sm font-semibold text-tx-0">
                  {connectorName(connectors.data ?? [], delivery.connector_id)}
                </div>
                <dl className="mt-2 grid gap-1 text-xs">
                  <div className="flex gap-2">
                    <dt className="w-24 shrink-0 text-tx-3">{t('deliveries.target')}</dt>
                    <dd className="min-w-0 truncate font-mono text-tx-1">
                      {delivery.target_value_masked ?? '—'}
                    </dd>
                  </div>
                  {delivery.error_message && (
                    <div className="flex gap-2">
                      <dt className="w-24 shrink-0 text-tx-3">{t('deliveries.error')}</dt>
                      <dd className="text-red-soft">{delivery.error_message}</dd>
                    </div>
                  )}
                </dl>
              </div>
            </div>
          ))}
        </div>
      </FormDrawer>
    </>
  );
}
