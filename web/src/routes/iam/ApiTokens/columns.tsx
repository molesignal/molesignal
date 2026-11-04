import { Trash2 } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { DataTableColumn } from '@/admin';
import type { ApiToken } from '@/api/apiTokens';
import type { ActionAccess } from '@/product/actionAccess';
import { IconButton } from '@/shell/chrome';
import { StatusWord } from '@/shell/StatusWord';

import { tokenRisks } from './risk';
import { TokenScopeLabel, useTokenScopeWords } from './scope';
import { formatMicros } from '../../rum/_helpers';

function displayPrefix(token: ApiToken): string {
  return `mspat-${token.prefix}`;
}

function formatOptionalMicros(value: number | null | undefined, fallback: string): string {
  return value ? formatMicros(value) : fallback;
}

/** The widths below leave the name 200px or more, with and without who the token acts as. */
export const TOKEN_TABLE_MIN_WIDTH = { all: 1040, own: 820 } as const;

/**
 * The columns every token list shows. The prefix and the kind stand under the
 * name, so that what needs a look and the revoke action are on screen beside it.
 * A list puts who the token acts as after the name when that varies, says whether
 * a token is revoked when it lists revoked ones, and ends with the revoke action.
 */
export function useTokenColumns({
  principal,
  showStatus = true,
  revoke,
}: {
  principal?: DataTableColumn<ApiToken>;
  showStatus?: boolean;
  revoke: {
    access: ActionAccess;
    pending: boolean;
    onRevoke: (token: ApiToken) => void;
  };
}): DataTableColumn<ApiToken>[] {
  const { t } = useTranslation('iam');
  const { t: tc } = useTranslation('common');
  const nowMicros = Date.now() * 1000;
  const scopeWords = useTokenScopeWords();
  const kindWords = (token: ApiToken) =>
    `${t(`api_tokens.kinds.${token.tokenKind}`)}${token.applicationId ? ` · ${token.applicationId}` : ''}`;
  const statusWords = (token: ApiToken) =>
    token.revoked ? t('api_tokens.status_revoked') : t('api_tokens.status_active');

  return [
    {
      key: 'name',
      header: t('api_tokens.columns.name'),
      sortable: true,
      value: (token) => token.name,
      cell: (token) => (
        <div className="min-w-0" title={token.name}>
          <div className="truncate text-tx-0">{token.name}</div>
          <div className="truncate text-tx-2">
            {displayPrefix(token)}… · {kindWords(token)}
          </div>
        </div>
      ),
    },
    {
      key: 'prefix',
      header: t('api_tokens.columns.prefix'),
      fileOnly: true,
      value: displayPrefix,
      cell: displayPrefix,
    },
    {
      key: 'kind',
      header: t('api_tokens.columns.kind'),
      fileOnly: true,
      value: kindWords,
      cell: kindWords,
    },
    ...(principal ? [principal] : []),
    {
      key: 'scope',
      header: t('api_tokens.columns.scope'),
      value: scopeWords,
      cell: (token) => <TokenScopeLabel token={token} />,
      width: 120,
    },
    ...(showStatus
      ? [
          {
            key: 'status',
            header: t('api_tokens.columns.status'),
            value: statusWords,
            cell: (token: ApiToken) => (
              <StatusWord tone={token.revoked ? 'dim' : 'green'}>{statusWords(token)}</StatusWord>
            ),
            width: 72,
          },
        ]
      : []),
    {
      key: 'expires',
      header: t('api_tokens.columns.expires'),
      sortable: true,
      value: (token) => formatOptionalMicros(token.expiresAt, t('api_tokens.never_expires')),
      sortValue: (token) => token.expiresAt ?? null,
      cell: (token) => formatOptionalMicros(token.expiresAt, t('api_tokens.never_expires')),
      width: 150,
    },
    {
      key: 'last_used',
      header: t('api_tokens.columns.last_used'),
      sortable: true,
      value: (token) => formatOptionalMicros(token.lastUsedAt, ''),
      sortValue: (token) => token.lastUsedAt ?? null,
      cell: (token) => formatOptionalMicros(token.lastUsedAt, '—'),
      width: 150,
    },
    {
      key: 'risk',
      header: t('api_tokens.columns.risk'),
      value: (token) =>
        tokenRisks(token, nowMicros)
          .map((risk) => t(`api_tokens.risks.${risk}`))
          .join('; '),
      cell: (token) => {
        const risks = tokenRisks(token, nowMicros);
        if (risks.length === 0) return <span className="text-tx-3">—</span>;

        return (
          <span data-token-risks className="flex flex-col gap-0.5">
            {risks.map((risk) => (
              <StatusWord key={risk} tone="orange">
                {t(`api_tokens.risks.${risk}`)}
              </StatusWord>
            ))}
          </span>
        );
      },
      width: 124,
    },
    {
      key: 'actions',
      header: t('api_tokens.columns.actions'),
      width: 72,
      cell: (token) => (
        <IconButton
          disabled={revoke.access.disabled || token.revoked || revoke.pending}
          disabledReason={
            revoke.access.reason ??
            (token.revoked
              ? t('api_tokens.already_revoked')
              : revoke.pending
                ? tc('access.operation_pending')
                : undefined)
          }
          onClick={() => revoke.onRevoke(token)}
          className="enabled:hover:bg-red-dim enabled:hover:text-red-soft"
          aria-label={t('api_tokens.revoke_token', { name: token.name })}
        >
          <Trash2 className="h-3 w-3" />
        </IconButton>
      ),
    },
  ];
}
