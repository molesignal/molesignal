import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { RefreshCw, Upload, UserMinus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import { ConfirmDialog, DataTable } from '@/admin';
import * as statusPagesApi from '@/api/statusPages';
import type { StatusPageSubscriber } from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import { PageBody } from '@/shell/PageHeader';
import { ResultPagination } from '@/shell/ResultPagination';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { toast } from '@/shell/ui/sonner';

import { useStatusPageWorkspace } from '../Layout';
import { shouldShowStatusPagePagination } from '../pagination';
import {
  StatusPageCanvas,
  StatusPageList,
  statusPageFlatTableClassName,
} from '../Surfaces';
import { ImportDrawer } from './ImportDrawer';
import { subscriberComponentNames } from './scope';

export function StatusPageSubscribers({ view }: { view: 'list' | 'deliveries' }) {
  const { t } = useTranslation('status-pages');
  const queryClient = useQueryClient();
  const [searchParams, setSearchParams] = useSearchParams();
  const { pageId, snapshot, manageAccess } = useStatusPageWorkspace();
  const [revoking, setRevoking] = React.useState<StatusPageSubscriber | null>(null);
  const [importing, setImporting] = React.useState(false);
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
  const canImport = manageAccess.allowed && snapshot.page.lifecycle === 'active';

  return (
    <PageBody className="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2">
      <StatusPageCanvas>
        <StatusPageList
          head={
            <>
              <ViewTabs
                label={t('subscriber_views_label')}
                items={(['list', 'deliveries'] as const).map((item) => ({
                  key: item,
                  to: `/status-pages/${pageId}/subscribers/${item}`,
                  label: t(`subscriber_views.${item}`),
                }))}
              />
              {/* How long the records below are kept is a fact about the deliveries, not the subscribers. */}
              {view === 'deliveries' && (
                <p className="ml-auto text-xs text-tx-2">
                  {t('subscribers.retention_hint', { count: snapshot.page.delivery_retention_days })}
                </p>
              )}
            </>
          }
          {...(view === 'list'
            ? {
                action: (
                  <ChromeButton
                    disabled={!canImport}
                    disabledReason={manageAccess.reason}
                    onClick={() => setImporting(true)}
                  >
                    <Upload className="h-3.5 w-3.5" />
                    {t('actions.import_subscribers')}
                  </ChromeButton>
                ),
              }
            : {})}
        >
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
                  <div className="flex min-w-0 items-center gap-2">
                    {/* A Slack channel added with Add to Slack is known by its name, not its address. */}
                    {subscriber.target_label ? (
                      <span className="truncate text-xs text-tx-1">{subscriber.target_label}</span>
                    ) : (
                      <span className="truncate font-mono text-xs text-tx-1">{subscriber.masked_target}</span>
                    )}
                    {subscriber.origin && subscriber.origin !== 'page' && (
                      <Pill tone="dim">{t(`subscriber_origin.${subscriber.origin}`)}</Pill>
                    )}
                  </div>
                ),
              },
              {
                key: 'channel',
                header: t('columns.channel'),
                width: 120,
                cell: (subscriber) => t(`channels.${subscriber.channel}`),
              },
              {
                key: 'scope',
                header: t('columns.subscription_scope'),
                width: 220,
                cell: (subscriber) => {
                  const names = subscriberComponentNames(subscriber, snapshot.components);
                  return names.length === 0 ? (
                    <span className="text-tx-2">{t('subscribers.scope_all')}</span>
                  ) : (
                    <span className="block truncate text-tx-1" title={names.join(', ')}>
                      {names.join(', ')}
                    </span>
                  );
                },
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
        </StatusPageList>
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
      <ImportDrawer
        open={importing}
        pageId={pageId}
        onOpenChange={setImporting}
        onImported={refresh}
      />
    </PageBody>
  );
}
