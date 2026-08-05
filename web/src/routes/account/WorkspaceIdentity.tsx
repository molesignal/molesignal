import { useQuery, useQueryClient } from '@tanstack/react-query';
import { ChevronDown } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as meApi from '@/api/me';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { AccountSection } from '@/routes/account/AccountSection';
import { CopyableValue } from '@/routes/settings/_atoms';
import { ChromeButton, Pill } from '@/shell/chrome';
import { DisabledControl } from '@/shell/DisabledControl';
import { toast } from '@/shell/ui/sonner';
import { normalizeRole } from '@/stores/auth';
import {
  useCurrentOrgSelection,
  useOrgStore,
} from '@/stores/useOrgStore';

export function AccountWorkspaceIdentity() {
  const { t } = useTranslation(['account', 'common']);
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const profileQuery = useQuery({
    queryKey: ['me', 'profile'],
    queryFn: () => meApi.profile(),
  });
  const { orgOptions, currentOrgId } = useCurrentOrgSelection();
  const loaded = useOrgStore((state) => state.loaded);
  const loadOrgs = useOrgStore((state) => state.loadOrgs);
  const switchOrg = useOrgStore((state) => state.switchOrg);
  const loading = useOrgStore((state) => state.loading);
  const [switching, setSwitching] = React.useState<string | null>(null);
  const apiTokensAccess = useActionAccess({
    permission: 'api_tokens.read',
  });

  React.useEffect(() => {
    if (!loaded) {
      void loadOrgs().catch(() => {
        // The current workspace from the auth context remains usable when the
        // optional workspace list cannot be loaded.
      });
    }
  }, [loadOrgs, loaded]);

  const selectWorkspace = async (id: string) => {
    if (id === currentOrgId || switching) return;
    setSwitching(id);
    try {
      await switchOrg(id, { queryClient });
      navigate('/home');
    } catch (error) {
      toast.error(toApiError(error).message);
    } finally {
      setSwitching(null);
    }
  };
  const profile = profileQuery.data;

  return (
    <AccountSection
      title={t('workspace.title')}
      subtitle={t('workspace.subtitle')}
      width="page"
    >
      <div className="overflow-hidden rounded-md bg-bg-1">
        <div className="grid grid-cols-[minmax(180px,1fr)_120px_100px] gap-3 bg-bg-2 px-3 py-2 font-sans text-xs font-strong text-tx-2">
          <span>{t('workspace.columns.workspace')}</span>
          <span>{t('workspace.columns.role')}</span>
          <span>{t('workspace.columns.status')}</span>
        </div>
        {orgOptions.map((org) => {
          const current = org.id === currentOrgId;
          const rowDisabled = current || org.disabled || loading || Boolean(switching);
          const rowDisabledReason = org.disabled
            ? t('workspace.organization_disabled')
            : current
              ? t('workspace.current_disabled')
            : switching
              ? t('workspace.switching_disabled')
              : loading
                ? t('common:access.loading')
                : undefined;
          return (
            <DisabledControl
              key={org.id}
              disabled={rowDisabled}
              reason={rowDisabledReason}
              className="w-full"
            >
              <button
                type="button"
                disabled={rowDisabled}
                aria-disabled={rowDisabled || undefined}
                onClick={() => void selectWorkspace(org.id)}
                className="grid min-h-12 w-full grid-cols-[minmax(180px,1fr)_120px_100px] items-center gap-3 px-3 text-left enabled:hover:bg-bg-2 disabled:cursor-not-allowed disabled:bg-bg-2/40 disabled:opacity-60"
              >
                <span className="min-w-0">
                  <span className="block truncate font-sans text-sm font-strong text-tx-0">
                    {org.name}
                  </span>
                  {org.slug && (
                    <span className="block truncate font-mono text-xs text-tx-3">
                      {org.slug}
                    </span>
                  )}
                </span>
                <span className="font-sans text-xs text-tx-1">
                  {org.display_role
                    ? normalizeRole(org.display_role)
                    : current
                      ? normalizeRole(profile?.display_role)
                      : '—'}
                </span>
                <span>
                  {org.disabled ? (
                    <Pill tone="dim">{t('common:status.disabled')}</Pill>
                  ) : current ? (
                    <Pill tone="green">{t('workspace.current')}</Pill>
                  ) : switching === org.id ? (
                    <Pill tone="dim">{t('workspace.switching')}</Pill>
                  ) : (
                    <span className="font-sans text-xs text-indigo-soft">
                      {t('workspace.switch')}
                    </span>
                  )}
                </span>
              </button>
            </DisabledControl>
          );
        })}
      </div>

      {profile && (
        <details className="group mt-6">
          <summary className="flex min-h-12 cursor-pointer list-none items-center gap-2 py-3 font-sans text-sm font-strong text-tx-0">
            <ChevronDown className="h-4 w-4 text-tx-3 transition-transform group-open:rotate-180" />
            {t('workspace.advanced')}
          </summary>
          <div className="grid gap-4 pt-4 sm:grid-cols-2">
            <div>
              <div className="mb-1.5 font-sans text-xs font-strong text-tx-2">
                {t('workspace.user_id')}
              </div>
              <CopyableValue
                value={profile.user_id}
                copyLabel={t('workspace.copy')}
                copiedLabel={t('workspace.copied')}
              />
            </div>
            <div>
              <div className="mb-1.5 font-sans text-xs font-strong text-tx-2">
                {t('workspace.org_id')}
              </div>
              <CopyableValue
                value={profile.org_id}
                copyLabel={t('workspace.copy')}
                copiedLabel={t('workspace.copied')}
              />
            </div>
            <div className="sm:col-span-2">
              <div className="mb-1.5 font-sans text-xs font-strong text-tx-2">
                {t('workspace.org_slug')}
              </div>
              <CopyableValue
                value={profile.org_slug}
                copyLabel={t('workspace.copy')}
                copiedLabel={t('workspace.copied')}
              />
              <p className="mt-1.5 font-sans text-xs text-tx-3">
                {t('workspace.org_slug_hint')}
              </p>
            </div>
          </div>
        </details>
      )}

      <div className="mt-6 flex flex-wrap items-center gap-2">
        <ChromeButton
          disabled={apiTokensAccess.disabled}
          disabledReason={apiTokensAccess.reason}
          onClick={() => navigate('/iam/service-accounts')}
        >
          {t('workspace.api_tokens')}
        </ChromeButton>
      </div>
    </AccountSection>
  );
}
