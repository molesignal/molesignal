import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as apiTokens from '@/api/apiTokens';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { AccountSection } from '@/routes/account/AccountSection';
import { TOKEN_TABLE_MIN_WIDTH, useTokenColumns } from '@/routes/iam/ApiTokens/columns';
import {
  CreateApiTokenDrawer,
  TOKEN_LIST_KEYS,
} from '@/routes/iam/ApiTokens/CreateApiTokenDrawer';
import { ChromeButton } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

/** The signed-in user's own tokens, which act as them and never do more than they may. */
export function AccountApiTokens() {
  const { t } = useTranslation('account');
  const { t: tIam } = useTranslation('iam');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'personal_tokens.manage' });
  const [creating, setCreating] = React.useState(false);
  const query = useQuery({
    queryKey: TOKEN_LIST_KEYS.me,
    queryFn: apiTokens.listMine,
    enabled: access.allowed,
  });
  // A revoked token is gone for its owner; the organization's list keeps it.
  const rows = (query.data ?? []).filter((token) => !token.revoked);
  const revoke = useMutation({
    mutationFn: (token: apiTokens.ApiToken) => apiTokens.revokeMine(token.id),
    onSuccess: async (_result, token) => {
      toast.success(tIam('api_tokens.toast_revoked'));
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: TOKEN_LIST_KEYS.me }),
        queryClient.invalidateQueries({ queryKey: TOKEN_LIST_KEYS.service_account }),
        ...(token.tokenKind === 'default_intake'
          ? [queryClient.invalidateQueries({ queryKey: ['default-intake-token'] })]
          : []),
      ]);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const columns = useTokenColumns({
    showStatus: false,
    revoke: {
      access,
      pending: revoke.isPending,
      onRevoke: (token) => revoke.mutate(token),
    },
  });
  const state = productStateFor(
    queryStateFor({ isLoading: query.isLoading, isError: query.isError, data: rows }),
    {
      error: query.error,
      emptyTitle: t('api_tokens.empty_title'),
      emptyDescription: t('api_tokens.empty_description'),
    },
  );

  return (
    <AccountSection
      title={t('api_tokens.title')}
      subtitle={t('api_tokens.subtitle')}
      width="table"
      actions={
        <ChromeButton
          variant="primary"
          onClick={() => setCreating(true)}
          disabled={access.disabled}
          disabledReason={access.reason}
        >
          <Plus className="h-3 w-3" />
          {t('api_tokens.create')}
        </ChromeButton>
      }
    >
      {state ? (
        <ProductState {...state} />
      ) : (
        <DataTable
          rows={rows}
          rowKey={(token) => token.id}
          columns={columns}
          fixedMinWidth={TOKEN_TABLE_MIN_WIDTH.own}
        />
      )}
      <CreateApiTokenDrawer
        open={creating}
        access={access}
        principal="me"
        onClose={() => setCreating(false)}
      />
    </AccountSection>
  );
}
