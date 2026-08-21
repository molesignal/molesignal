import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Outlet, useLocation, useNavigate } from 'react-router-dom';

import {
  canAccessProductPath,
  useProductAccess,
} from '@/product/access';
import { ProductRouteAccessGuard } from '@/routes/RouteGuard';
import { InvestigationContextBar } from '@/shell/InvestigationContextBar';
import { cn } from '@/shell/lib/cn';
import { MoleAgentPanel } from '@/shell/MoleAgentPanel';
import { Sidebar } from '@/shell/Sidebar';
import { Topbar } from '@/shell/Topbar';
import { UnsupportedScreen } from '@/shell/UnsupportedScreen';
import {
  DESKTOP_MIN_WIDTH,
  useViewportWidth,
} from '@/shell/useViewportWidth';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';

interface AppShellProps {
  onTimePickerOpen: () => void;
  onPaletteOpen: () => void;
}

const SURFACE_WORKBENCH_ROUTES = [
  '/dashboards',
  '/logs',
  '/metrics',
  '/traces',
  '/apm',
  '/rum',
  '/profiles',
  '/alerts',
  '/synthetics',
  '/status-pages',
  '/agent',
  '/datasource',
  '/streams',
  '/pipelines',
  '/functions',
  '/extend-tables',
  '/reports',
  '/iam',
  '/settings',
  '/account',
] as const;

function isSurfaceWorkbenchRoute(pathname: string): boolean {
  return SURFACE_WORKBENCH_ROUTES.some(
    (route) => pathname === route || pathname.startsWith(`${route}/`),
  );
}

/**
 * Shell layout: fixed Topbar / collapsible Sidebar / fluid main.
 * The chrome regions are fixed-positioned; the `<main>`
 * element is offset by topbar+sidebar via padding so scrollable page bodies
 * don't slide under the chrome. The InvestigationContextBar (when an
 * investigation is active) and the Mole Agent slide-out are mounted here at
 * the shell level so they persist across route changes.
 */
export function AppShell(_props: AppShellProps) {
  const { t } = useTranslation('shell');
  const [collapsed, setCollapsed] = React.useState(true);
  const [temporarilyExpanded, setTemporarilyExpanded] = React.useState(false);
  const [mobileNavOpen, setMobileNavOpen] = React.useState(false);
  const location = useLocation();
  const [autoCollapsedSidebarExpanded, setAutoCollapsedSidebarExpanded] = React.useState(false);
  const isSettingsRoute =
    location.pathname === '/settings' || location.pathname.startsWith('/settings/');
  const isIamRoute = location.pathname === '/iam' || location.pathname.startsWith('/iam/');
  const isManagementRoute = isSettingsRoute || isIamRoute;
  const isDashboardEditorRoute =
    location.pathname === '/dashboards/new/edit' ||
    /^\/dashboards\/[^/]+\/edit$/.test(location.pathname) ||
    /^\/dashboards\/[^/]+\/panels\/new$/.test(location.pathname);
  const isAutoCollapsedRoute = isManagementRoute || isDashboardEditorRoute;
  const isSurfaceWorkbench = isSurfaceWorkbenchRoute(location.pathname);
  const primarySidebarCollapsed = isAutoCollapsedRoute
    ? !autoCollapsedSidebarExpanded
    : collapsed;
  const sidebarVisuallyCollapsed =
    primarySidebarCollapsed && !temporarilyExpanded;
  const nav = useNavigate();
  const viewportWidth = useViewportWidth();
  const toggleMoleAgent = useMoleAgentStore((s) => s.toggle);
  const access = useProductAccess();
  const canUseMoleAgent = canAccessProductPath('/agent', access);

  React.useEffect(() => {
    setMobileNavOpen(false);
  }, [location.pathname]);

  React.useEffect(() => {
    if (!isAutoCollapsedRoute) setAutoCollapsedSidebarExpanded(false);
  }, [isAutoCollapsedRoute]);

  React.useEffect(() => {
    if (!primarySidebarCollapsed) setTemporarilyExpanded(false);
  }, [primarySidebarCollapsed]);

  // ⌘J / Ctrl-J toggles Mole Agent from anywhere in the app.
  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (
        canUseMoleAgent &&
        (e.metaKey || e.ctrlKey) &&
        !e.shiftKey &&
        !e.altKey &&
        e.key.toLowerCase() === 'j'
      ) {
        e.preventDefault();
        toggleMoleAgent();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [canUseMoleAgent, toggleMoleAgent]);

  const handleToggleSidebar = () => {
    const isMobile = typeof window !== 'undefined' && window.innerWidth < 768;
    if (isMobile) {
      setMobileNavOpen((v) => !v);
      return;
    }
    setTemporarilyExpanded(false);
    if (isAutoCollapsedRoute) {
      setAutoCollapsedSidebarExpanded((v) => !v);
      return;
    }
    setCollapsed((v) => !v);
  };

  // The product contract is desktop-only below 1024px, including Settings and
  // IAM. Keeping one shell-wide threshold avoids suggesting that a narrow
  // management drawer makes dense administrative tables mobile-supported.
  // Hooks above run unconditionally so this early return stays hook-safe.
  if (viewportWidth < DESKTOP_MIN_WIDTH) {
    return <UnsupportedScreen width={viewportWidth} />;
  }

  return (
    <div
      data-shell-layout={isSurfaceWorkbench ? 'surface-workbench' : undefined}
      className={cn(
        'h-screen min-w-0 overflow-hidden bg-bg-0 text-tx-0',
        isSurfaceWorkbench && 'bg-[var(--page-canvas)] [--sidebar-w:216px] [--topbar-h:48px]',
      )}
    >
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:left-2 focus:top-2 focus:z-[100] focus:rounded focus:bg-indigo focus:px-2 focus:py-1 focus:text-white"
      >
        Skip to content
      </a>

      <Topbar
        onToggleSidebar={handleToggleSidebar}
        onPaletteOpen={_props.onPaletteOpen}
        onNocOpen={() => nav('/noc')}
        surfaceWorkbench={isSurfaceWorkbench}
      />

      {mobileNavOpen && (
        <button
          type="button"
          aria-label={t('chrome.close_navigation')}
          className="fixed inset-x-0 bottom-0 top-topbar z-30 bg-overlay md:hidden"
          onClick={() => setMobileNavOpen(false)}
        />
      )}

      <Sidebar
        collapsed={sidebarVisuallyCollapsed}
        mobileOpen={mobileNavOpen}
        onNavigate={() => setMobileNavOpen(false)}
        onHoverChange={(hovered) =>
          setTemporarilyExpanded(primarySidebarCollapsed && hovered)
        }
        surfaceWorkbench={isSurfaceWorkbench}
      />

      <main
        id="main"
        className={cn(
          'h-screen min-w-0 overflow-x-hidden overflow-y-auto pt-topbar transition-[padding-left] duration-normal ease-out-default',
          primarySidebarCollapsed ? 'md:pl-sidebar-collapsed' : 'md:pl-sidebar',
        )}
      >
        <InvestigationContextBar />
        <ProductRouteAccessGuard>
          <Outlet />
        </ProductRouteAccessGuard>
      </main>

      {canUseMoleAgent && <MoleAgentPanel />}
    </div>
  );
}
