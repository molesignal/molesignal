import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin/ConfirmDialog';
import * as ssoApi from '@/api/sso';
import { toApiError } from '@/lib/http';
import { useTimeFormatter } from '@/lib/time';
import { ChromeButton, Pill } from '@/shell/chrome';
import { SsoPlatformMark } from '@/shell/SsoPlatformMark';
import { toast } from '@/shell/ui/sonner';

const LINKED_QUERY_KEY = ['me', 'sso-identities'] as const;

/**
 * The single sign-on identities that sign in to this account. Unlinking one stops it
 * signing in here; signing in with it again asks the owner to confirm, as the first time.
 */
export function LinkedSignIns() {
  const { t } = useTranslation(['account', 'common']);
  const time = useTimeFormatter();
  const queryClient = useQueryClient();
  const [removing, setRemoving] = React.useState<ssoApi.LinkedIdentity | null>(null);
  const linkedQuery = useQuery({
    queryKey: LINKED_QUERY_KEY,
    queryFn: () => ssoApi.listLinkedIdentities(),
  });
  const unlink = useMutation({
    mutationFn: (identity: ssoApi.LinkedIdentity) => ssoApi.unlinkIdentity(identity.id),
    onSuccess: (_, identity) => {
      toast.success(t('account:security.linked.unlinked', { provider: identity.providerName }));
      setRemoving(null);
      void queryClient.invalidateQueries({ queryKey: LINKED_QUERY_KEY });
    },
    onError: (error) =>
      toast.error(t('account:security.linked.unlink_failed'), {
        description: toApiError(error).message,
      }),
  });
  const identities = linkedQuery.data ?? [];

  return (
    <div className="py-4" data-testid="linked-sign-ins">
      <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
        <div>
          <div className="font-sans text-sm font-strong text-tx-0">
            {t('account:security.linked.title')}
          </div>
          <div className="mt-1 max-w-xl font-sans text-xs leading-relaxed text-tx-2">
            {t('account:security.linked.description')}
          </div>
        </div>
        {linkedQuery.isError ? (
          <span className="font-sans text-xs text-red-soft">
            {t('account:security.linked.load_failed')}
          </span>
        ) : (
          linkedQuery.isSuccess &&
          identities.length === 0 && (
            <span aria-readonly="true">
              <Pill tone="dim">{t('account:security.linked.none')}</Pill>
            </span>
          )
        )}
      </div>

      {identities.length > 0 && (
        <ul className="mt-3 flex flex-col gap-2">
          {identities.map((identity) => (
            <li key={identity.id} className="flex items-center gap-3">
              <div className="grid h-9 w-9 shrink-0 place-items-center rounded-md bg-bg-2">
                <SsoPlatformMark platform={identity.platform} kind={identity.kind} />
              </div>
              <div className="min-w-0 flex-1">
                <div className="truncate font-sans text-sm text-tx-0">{identity.providerName}</div>
                <div className="mt-0.5 truncate font-sans text-xs text-tx-2">
                  {identity.orgName}
                  {' · '}
                  {t('account:security.linked.last_used', {
                    time: time.micros(identity.lastLoginAt, false),
                  })}
                </div>
              </div>
              <ChromeButton
                disabled={!identity.canUnlink || unlink.isPending}
                disabledReason={
                  identity.canUnlink ? undefined : t('account:security.linked.only_way')
                }
                onClick={() => setRemoving(identity)}
              >
                {t('account:security.linked.unlink')}
              </ChromeButton>
            </li>
          ))}
        </ul>
      )}

      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('account:security.linked.unlink_title', {
          provider: removing?.providerName ?? '',
        })}
        description={t('account:security.linked.unlink_description', {
          provider: removing?.providerName ?? '',
          organization: removing?.orgName ?? '',
        })}
        confirmLabel={t('account:security.linked.unlink')}
        cancelLabel={t('common:actions.cancel')}
        busy={unlink.isPending}
        busyLabel={t('account:security.linked.unlinking')}
        onConfirm={() => removing && unlink.mutate(removing)}
      />
    </div>
  );
}
