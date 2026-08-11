import { useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowLeft, ExternalLink, Globe2, LockKeyhole } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import {
  Navigate,
  NavLink,
  Outlet,
  useLocation,
  useOutletContext,
  useParams,
} from 'react-router-dom';

import * as statusPagesApi from '@/api/statusPages';
import type {
  StatusPageDomainCapability,
  StatusPageDomainInstructions,
  StatusPageSnapshot,
} from '@/api/statusPages';
import { hasPermission, useProductAccess } from '@/product/access';
import { useActionAccess, type ActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { PageBody, PageHeader } from '@/shell/PageHeader';

import { publicStatusPageUrl } from '../publicUrl';

interface WorkspaceContextValue {
  pageId: string;
  snapshot: StatusPageSnapshot;
  domain: StatusPageDomainInstructions | null | undefined;
  domainCapability: StatusPageDomainCapability | undefined;
  manageAccess: ActionAccess;
  refresh: () => Promise<void>;
}

const TABS = [
  'overview',
  'components',
  'incidents',
  'maintenance',
  'history',
  'subscribers',
  'settings',
] as const;

export function StatusPageWorkspaceLayout() {
  const { pageId = '' } = useParams();
  const { t } = useTranslation('status-pages');
  const location = useLocation();
  const queryClient = useQueryClient();
  const productAccess = useProductAccess();
  const manageAccess = useActionAccess({ permission: 'status_pages.manage' });
  const snapshotQuery = useQuery({
    queryKey: ['status-pages', pageId, 'snapshot'],
    queryFn: () => statusPagesApi.get(pageId),
    enabled: Boolean(pageId),
  });
  const domainCapabilityQuery = useQuery({
    queryKey: ['status-pages', pageId, 'domain-capability'],
    queryFn: () => statusPagesApi.getDomainCapability(pageId),
    enabled: Boolean(pageId) && manageAccess.allowed,
  });
  const domainQuery = useQuery({
    queryKey: ['status-pages', pageId, 'domain'],
    queryFn: () => statusPagesApi.getDomain(pageId),
    enabled:
      Boolean(pageId) && manageAccess.allowed && domainCapabilityQuery.data?.available === true,
  });

  if (snapshotQuery.isLoading) {
    return (
      <PageBody>
        <ProductState variant="loading" />
      </PageBody>
    );
  }
  if (!snapshotQuery.data || snapshotQuery.isError) {
    return (
      <PageBody>
        <ProductState variant="error" error={snapshotQuery.error} />
      </PageBody>
    );
  }

  const isManageOnlyRoute = /\/(subscribers|settings)(?:\/|$)/.test(location.pathname);
  if (
    isManageOnlyRoute
    && productAccess?.status === 'ready'
    && !hasPermission('status_pages.manage', productAccess)
  ) {
    return <Navigate to={`/status-pages/${pageId}/overview`} replace />;
  }

  const snapshot = snapshotQuery.data;
  const { page } = snapshot;
  const publicUrl = publicStatusPageUrl(page, domainQuery.data?.config.state);
  const visibleTabs = manageAccess.allowed
    ? TABS
    : TABS.filter((tab) => tab !== 'subscribers' && tab !== 'settings');
  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ['status-pages', pageId] }),
      queryClient.invalidateQueries({ queryKey: ['status-pages'] }),
    ]);
  };
  const context: WorkspaceContextValue = {
    pageId,
    snapshot,
    domain: domainQuery.data,
    domainCapability: domainCapabilityQuery.data,
    manageAccess,
    refresh,
  };

  return (
    <div className="min-w-0">
      <PageHeader
        title={page.name}
        subtitle={t('workspace.subtitle', { url: publicUrl })}
        breadcrumbs={null}
        backTo={null}
        toolbar={
          <div className="flex items-center gap-2">
            <Pill tone={page.visibility === 'public' ? 'green' : 'dim'}>
              {page.visibility === 'public' ? (
                <Globe2 className="h-3 w-3" />
              ) : (
                <LockKeyhole className="h-3 w-3" />
              )}
              {t(`visibility.${page.visibility}`)}
            </Pill>
            {page.lifecycle === 'active' && (
              <ChromeButton
                size="sm"
                onClick={() => window.open(publicUrl, '_blank', 'noopener,noreferrer')}
              >
                <ExternalLink className="h-3.5 w-3.5" />
                {t('actions.open_status_page')}
              </ChromeButton>
            )}
          </div>
        }
      />
      <div className="flex min-w-0 items-center border-b border-bd-0 bg-bg-1 px-3">
        <NavLink
          to="/status-pages"
          aria-label={t('actions.all_pages')}
          className="mr-2 grid h-10 w-9 shrink-0 place-items-center rounded-md text-tx-2 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2"
        >
          <ArrowLeft className="h-4 w-4" />
        </NavLink>
        <nav
          aria-label={t('tabs.label')}
          className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto overflow-y-hidden"
        >
          {visibleTabs.map((tab) => (
            <NavLink
              key={tab}
              to={`/status-pages/${pageId}/${tab}`}
              className={({ isActive }) =>
                cn(
                  'inline-flex h-11 shrink-0 items-center whitespace-nowrap border-b-2 px-3 text-xs font-strong transition-colors',
                  isActive
                    ? 'border-indigo text-tx-0'
                    : 'border-transparent text-tx-2 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2',
                )
              }
            >
              {t(`tabs.${tab}`)}
            </NavLink>
          ))}
        </nav>
      </div>
      {page.lifecycle === 'archived' && (
        <div className="border-b border-yellow/25 bg-yellow-dim px-6 py-2 text-xs text-yellow-soft">
          {t('states.archived_read_only')}
        </div>
      )}
      <Outlet context={context} />
    </div>
  );
}

export function useStatusPageWorkspace(): WorkspaceContextValue {
  return useOutletContext<WorkspaceContextValue>();
}
