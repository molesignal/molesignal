import { useQuery, useQueryClient } from '@tanstack/react-query';
import { ExternalLink, Globe2, LockKeyhole } from 'lucide-react';
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
import {
  surfaceModuleNavigationActiveClass,
  surfaceModuleNavigationClass,
  surfaceModuleNavigationItemClass,
  surfaceModuleNavigationRowClass,
} from '@/shell/SurfaceWorkbench';

import { publicStatusPageUrl } from '../publicUrl';
import {
  StatusPageCanvas,
  statusPageFlatStateClassName,
} from './CardlessSurface';

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
      <PageBody className="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2">
        <StatusPageCanvas>
          <ProductState variant="loading" className={statusPageFlatStateClassName} />
        </StatusPageCanvas>
      </PageBody>
    );
  }
  if (!snapshotQuery.data || snapshotQuery.isError) {
    return (
      <PageBody className="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2">
        <StatusPageCanvas>
          <ProductState
            variant="error"
            error={snapshotQuery.error}
            className={statusPageFlatStateClassName}
          />
        </StatusPageCanvas>
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
    <div className="min-w-0 bg-[var(--page-canvas)]">
      <PageHeader
        title={page.name}
        subtitle={t('workspace.subtitle', { url: publicUrl })}
        breadcrumbs={[
          { labelKey: 'status_pages', to: '/status-pages' },
          { labelKey: 'status_page', label: page.name },
        ]}
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
      <div className={surfaceModuleNavigationClass}>
        <nav
          aria-label={t('tabs.label')}
          className={surfaceModuleNavigationRowClass}
        >
          {visibleTabs.map((tab) => (
            <NavLink
              key={tab}
              to={`/status-pages/${pageId}/${tab}`}
              className={({ isActive }) =>
                cn(
                  surfaceModuleNavigationItemClass,
                  'whitespace-nowrap',
                  isActive
                    ? surfaceModuleNavigationActiveClass
                    : 'text-tx-2',
                )
              }
            >
              {t(`tabs.${tab}`)}
            </NavLink>
          ))}
        </nav>
      </div>
      {page.lifecycle === 'archived' && (
        <div className="mx-[20px] mb-[12px] rounded-md bg-yellow-dim px-4 py-2 text-xs text-yellow-soft">
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
