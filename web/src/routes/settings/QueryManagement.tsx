import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import { DataTable, PageHeader } from '@/admin';
import * as runningApi from '@/api/runningQueries';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { SectionBody } from './_atoms';
import { formatMicros } from '../rum/_helpers';

export function QueryManagement() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const cancelAccess = useActionAccess({
    permission: 'org.settings.manage',
  });

  const q = useQuery({
    queryKey: ['running-queries'],
    queryFn: () => runningApi.list(),
    refetchInterval: 3_000,
  });
  const rows = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('query_management.empty_title'),
    emptyDescription: t('query_management.empty_description'),
  });

  const cancel = useMutation({
    mutationFn: (id: string) => runningApi.cancel(id),
    onSuccess: () => {
      toast.success(t('query_management.toast_cancelled'));
      void qc.invalidateQueries({ queryKey: ['running-queries'] });
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <>
      <PageHeader title={t('query_management.title')} subtitle={t('query_management.subtitle') as string} />
      <SectionBody>
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <DataTable
            rows={rows}
            rowKey={(r) => r.id}
            columns={[
              {
                key: 'id',
                header: t('query_management.columns.id'),
                cell: (r) => <span className="font-sans text-tx-0">{r.id.slice(0, 10)}</span>,
                width: 120,
              },
              {
                key: 'user',
                header: t('query_management.columns.user'),
                cell: (r) => r.user_id ?? '—',
                width: 200,
              },
              {
                key: 'started',
                header: t('query_management.columns.started'),
                cell: (r) => formatMicros(r.started_at_micros),
                width: 200,
              },
              {
                key: 'sql',
                header: t('query_management.columns.sql'),
                cell: (r) => (
                  <pre className="m-0 max-w-[480px] overflow-hidden text-ellipsis whitespace-nowrap font-sans text-xs text-tx-2">
                    {r.statement}
                  </pre>
                ),
              },
              {
                key: 'actions',
                header: '',
                width: 90,
                cell: (r) => {
                  const disabled =
                    cancelAccess.disabled ||
                    Boolean(r.cancelled) ||
                    cancel.isPending;
                  const reason =
                    cancelAccess.reason ??
                    (r.cancelled
                      ? t('query_management.already_cancelled')
                      : cancel.isPending
                        ? tc('access.operation_pending')
                        : undefined);
                  return (
                    <ChromeButton
                      size="sm"
                      onClick={() => {
                        if (!disabled) cancel.mutate(r.id);
                      }}
                      disabled={disabled}
                      disabledReason={reason}
                    >
                      {t('query_management.cancel')}
                    </ChromeButton>
                  );
                },
              },
            ]}
          />
        )}
      </SectionBody>
    </>
  );
}
