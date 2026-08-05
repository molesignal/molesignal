import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as usersApi from '@/api/users';
import { toApiError } from '@/lib/http';
import {
  restrictActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { IamListPage } from './IamLayout';

// 待审批：仅列 status=pending 的自助注册用户，行内通过 / 拒绝（拒绝走二次确认）。
// 与顶栏通知中心的「待审批」消息指向同一页。
export function Approvals() {
  const { t } = useTranslation('iam');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({
    permission: 'org.members.manage',
  });
  const [rejecting, setRejecting] = React.useState<usersApi.UserView | null>(null);

  const q = useQuery({
    queryKey: ['iam', 'users'],
    queryFn: () => usersApi.list(),
  });
  const rows = (q.data ?? []).filter((u) => u.status === 'pending');
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('approvals.empty_title'),
    emptyDescription: t('approvals.empty_description'),
  });

  const approve = useMutation({
    mutationFn: (id: string) => usersApi.approve(id),
    onSuccess: () => {
      toast.success(t('approvals.toast_approved'));
      void qc.invalidateQueries({ queryKey: ['iam', 'users'] });
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const reject = useMutation({
    mutationFn: (id: string) => usersApi.reject(id),
    onSuccess: () => {
      toast.success(t('approvals.toast_rejected'));
      void qc.invalidateQueries({ queryKey: ['iam', 'users'] });
      setRejecting(null);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const busy = approve.isPending || reject.isPending;

  return (
    <>
      <IamListPage
        title={t('approvals.title')}
        subtitle={t('approvals.subtitle') as string}
        state={pageState}
      >
        <DataTable
          rows={rows}
          rowKey={(r) => r.id}
          columns={[
            {
              key: 'user',
              header: t('approvals.columns.user'),
              cell: (r) => <span className="text-tx-0">{r.display_name || '—'}</span>,
            },
            { key: 'email', header: t('approvals.columns.email'), cell: (r) => r.email },
            {
              key: 'actions',
              header: '',
              width: 180,
              cell: (r) => {
                const actionAccess = restrictActionAccess(
                  manageAccess,
                  !busy,
                  t('approvals.action_pending'),
                );
                return (
                <div className="flex items-center justify-end gap-3">
                  <ChromeButton
                    size="sm"
                    variant="ghost"
                    disabled={actionAccess.disabled}
                    disabledReason={actionAccess.reason}
                    onClick={() => actionAccess.allowed && approve.mutate(r.id)}
                    className="text-green-soft enabled:hover:bg-green-dim"
                  >
                    {t('approvals.approve')}
                  </ChromeButton>
                  <ChromeButton
                    size="sm"
                    variant="ghost"
                    disabled={actionAccess.disabled}
                    disabledReason={actionAccess.reason}
                    onClick={() => actionAccess.allowed && setRejecting(r)}
                    className="text-red-soft enabled:hover:bg-red-dim"
                  >
                    {t('approvals.reject')}
                  </ChromeButton>
                </div>
                );
              },
            },
          ]}
        />
      </IamListPage>
      <ConfirmDialog
        open={rejecting !== null}
        onOpenChange={(v) => !v && setRejecting(null)}
        destructive
        title={t('approvals.reject_confirm_title')}
        description={t('approvals.reject_confirm_description')}
        confirmLabel={t('approvals.reject')}
        busy={reject.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (manageAccess.allowed && rejecting) reject.mutate(rejecting.id);
        }}
      />
    </>
  );
}
