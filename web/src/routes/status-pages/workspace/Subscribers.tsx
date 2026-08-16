import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { RefreshCw, UserMinus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { NavLink, useSearchParams } from 'react-router-dom';

import { ConfirmDialog, DataTable } from '@/admin';
import * as statusPagesApi from '@/api/statusPages';
import type { StatusPageSubscriber } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { IconButton, Pill } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { PageBody } from '@/shell/PageHeader';
import { ResultPagination } from '@/shell/ResultPagination';
import { toast } from '@/shell/ui/sonner';

import {
  StatusPageBand,
  StatusPageCanvas,
  StatusPageListSurface,
  statusPageFlatTableClassName,
} from './CardlessSurface';
import { useStatusPageWorkspace } from './Layout';
import { shouldShowStatusPagePagination } from './pagination';

export function StatusPageSubscribers({ view }: { view: 'list' | 'deliveries' }) {
  const { t } = useTranslation('status-pages');
  const queryClient = useQueryClient();
  const [searchParams, setSearchParams] = useSearchParams();
  const { pageId, snapshot, manageAccess } = useStatusPageWorkspace();
  const [revoking, setRevoking] = React.useState<StatusPageSubscriber | null>(null);
  const page = Math.max(1, Number(searchParams.get('page')) || 1);
  const subscribersQuery = useQuery({
    queryKey: ['status-pages', pageId, 'subscribers', page],
    queryFn: () => statusPagesApi.listSubscribers(pageId, page),
    enabled: view === 'list',
  });
  const deliveriesQuery = useQuery({
    queryKey: ['status-pages', pageId, 'deliveries', page],
    queryFn: () => statusPagesApi.listDeliveries(pageId, page),
    enabled: view === 'deliveries',
  });
  const refresh = () =>
    queryClient.invalidateQueries({ queryKey: ['status-pages', pageId] });
  const revokeMutation = useMutation({
    mutationFn: (id: string) => statusPagesApi.revokeSubscriber(pageId, id),
    onSuccess: async () => {
      toast.success(t('toast.subscriber_revoked'));
      setRevoking(null);
      await refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });
  const resendMutation = useMutation({
    mutationFn: (id: string) => statusPagesApi.resendSubscriber(pageId, id),
    onSuccess: async () => {
      toast.success(t('toast.confirmation_resent'));
      await refresh();
    },
    onError: (error: unknown) => toast.error(toApiError(error).message),
  });

  const result = view === 'list' ? subscribersQuery.data : deliveriesQuery.data;
  const loading = view === 'list' ? subscribersQuery.isLoading : deliveriesQuery.isLoading;
  const pageCount = Math.max(1, Math.ceil((result?.total ?? 0) / (result?.per_page ?? 25)));

  return (
    <PageBody className="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2">
      <StatusPageCanvas>
        <StatusPageBand className="flex flex-wrap items-center gap-2 py-0">
          <div className="flex items-center gap-1">
            {(['list', 'deliveries'] as const).map((item) => (
              <NavLink
                key={item}
                to={`/status-pages/${pageId}/subscribers/${item}`}
                className={({ isActive }) =>
                  cn(
                    'inline-flex h-12 items-center border-b-2 px-3 text-xs font-strong',
                    isActive
                      ? 'border-indigo text-tx-0'
                      : 'border-transparent text-tx-2 hover:bg-bg-2 hover:text-tx-0',
                  )
                }
              >
                {t(`subscriber_views.${item}`)}
              </NavLink>
            ))}
          </div>
          <p className="ml-auto text-xs text-tx-3">
            {t('subscribers.retention_hint', { count: snapshot.page.delivery_retention_days })}
          </p>
        </StatusPageBand>

        <StatusPageListSurface>
          {view === 'list' ? (
            <DataTable
            className={statusPageFlatTableClassName}
            rows={subscribersQuery.data?.items ?? []}
            rowKey={(subscriber) => subscriber.id}
            emptyLabel={loading ? t('states.loading') : t('states.no_subscribers')}
            columns={[
              {
                key: 'target',
                header: t('columns.subscriber'),
                cell: (subscriber) => (
                  <span className="font-mono text-xs text-tx-1">{subscriber.masked_target}</span>
                ),
              },
              {
                key: 'channel',
                header: t('columns.channel'),
                width: 120,
                cell: (subscriber) => t(`channels.${subscriber.channel}`),
              },
              {
                key: 'status',
                header: t('columns.status'),
                width: 130,
                cell: (subscriber) => (
                  <Pill tone={subscriber.status === 'active' ? 'green' : subscriber.status === 'pending' ? 'yellow' : 'dim'}>
                    {t(`subscriber_status.${subscriber.status}`)}
                  </Pill>
                ),
              },
              {
                key: 'created',
                header: t('columns.created'),
                width: 180,
                cell: (subscriber) => (
                  <span className="tabular-nums text-tx-3">{formatMicrosActive(subscriber.created_at)}</span>
                ),
              },
              {
                key: 'actions',
                header: t('columns.actions'),
                width: 110,
                cell: (subscriber) => (
                  <div className="flex items-center gap-0.5">
                    {subscriber.status === 'pending' && (
                      <IconButton
                        aria-label={t('actions.resend_confirmation')}
                        title={t('actions.resend_confirmation')}
                        disabled={
                          !manageAccess.allowed
                          || snapshot.page.lifecycle !== 'active'
                          || resendMutation.isPending
                        }
                        disabledReason={manageAccess.reason}
                        onClick={() => resendMutation.mutate(subscriber.id)}
                      >
                        <RefreshCw className="h-3.5 w-3.5" />
                      </IconButton>
                    )}
                    {subscriber.status !== 'unsubscribed' && (
                      <IconButton
                        aria-label={t('actions.revoke')}
                        title={t('actions.revoke')}
                        disabled={!manageAccess.allowed}
                        disabledReason={manageAccess.reason}
                        onClick={() => setRevoking(subscriber)}
                      >
                        <UserMinus className="h-3.5 w-3.5" />
                      </IconButton>
                    )}
                  </div>
                ),
              },
            ]}
            />
          ) : (
            <DataTable
            className={statusPageFlatTableClassName}
            rows={deliveriesQuery.data?.items ?? []}
            rowKey={(delivery) => delivery.id}
            emptyLabel={loading ? t('states.loading') : t('states.no_deliveries')}
            columns={[
              {
                key: 'event',
                header: t('columns.event'),
                cell: (delivery) => <span className="font-mono text-xs text-tx-1">{delivery.event_key}</span>,
              },
              {
                key: 'target',
                header: t('columns.subscriber'),
                cell: (delivery) => <span className="font-mono text-xs text-tx-2">{delivery.masked_target}</span>,
              },
              {
                key: 'status',
                header: t('columns.status'),
                width: 130,
                cell: (delivery) => (
                  <Pill tone={delivery.status === 'delivered' ? 'green' : delivery.status === 'failed' ? 'red' : 'yellow'}>
                    {t(`delivery_status.${delivery.status}`)}
                  </Pill>
                ),
              },
              {
                key: 'attempts',
                header: t('columns.attempts'),
                width: 90,
                cell: (delivery) => delivery.attempts,
              },
              {
                key: 'created',
                header: t('columns.created'),
                width: 180,
                cell: (delivery) => (
                  <span className="tabular-nums text-tx-3">{formatMicrosActive(delivery.created_at)}</span>
                ),
              },
            ]}
            />
          )}
          {shouldShowStatusPagePagination(result) && (
            <ResultPagination
              page={page}
              pageCount={pageCount}
              pageSize={result.per_page}
              pageSizeOptions={[25]}
              pageLabel={t('pagination.page', { page, pages: pageCount })}
              ariaLabel={t('pagination.label')}
              pageSizeAriaLabel={t('pagination.page_size')}
              firstAriaLabel={t('pagination.first')}
              previousAriaLabel={t('pagination.previous')}
              nextAriaLabel={t('pagination.next')}
              lastAriaLabel={t('pagination.last')}
              onPageChange={(next) => setSearchParams(next === 1 ? {} : { page: String(next) })}
              onPageSizeChange={() => undefined}
            />
          )}
        </StatusPageListSurface>
      </StatusPageCanvas>

      <ConfirmDialog
        open={Boolean(revoking)}
        onOpenChange={(open) => !open && setRevoking(null)}
        title={t('confirm.revoke_subscriber_title')}
        description={t('confirm.revoke_subscriber_description', { target: revoking?.masked_target })}
        confirmLabel={t('actions.revoke')}
        destructive
        busy={revokeMutation.isPending}
        onConfirm={() => revoking && revokeMutation.mutate(revoking.id)}
      />
    </PageBody>
  );
}
