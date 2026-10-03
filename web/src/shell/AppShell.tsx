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
import { Sheet, SheetContent, SheetTitle } from '@/shell/ui/sheet';
import { useViewportWidth } from '@/shell/useViewportWidth';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';

interface AppShellProps {
  onTimePickerOpen: () => void;
  onPaletteOpen: () => void;
}

const SURFACE_WORKBENCH_ROUTES = [
  '/home',
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

  React.useEffect(() => {
    if (viewportWidth >= 768) setMobileNavOpen(false);
  }, [viewportWidth]);

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

  return (
    <div
      data-shell-layout={isSurfaceWorkbench ? 'surface-workbench' : undefined}
      className={cn(
        'h-screen min-w-0 overflow-hidden bg-bg-0 text-tx-0',
        isSurfaceWorkbench && 'bg-[var(--page-canvas)] [--topbar-h:48px]',
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

      {viewportWidth < 768 ? (
        <Sheet open={mobileNavOpen} onOpenChange={setMobileNavOpen}>
          <SheetContent
            side="left"
            aria-describedby={undefined}
            className="w-[min(var(--sidebar-w),calc(100vw-32px))] p-0 pt-10"
            onCloseAutoFocus={(event) => {
              event.preventDefault();
              document.querySelector<HTMLButtonElement>('[data-testid="sidebar-toggle"]')?.focus();
            }}
          >
            <SheetTitle className="sr-only">{t('nav:primary_navigation')}</SheetTitle>
            <Sidebar
              collapsed={false}
              mobileOpen
              onNavigate={() => setMobileNavOpen(false)}
            />
          </SheetContent>
        </Sheet>
      ) : (
        <Sidebar
          collapsed={sidebarVisuallyCollapsed}
          onHoverChange={(hovered) =>
            setTemporarilyExpanded(primarySidebarCollapsed && hovered)
          }
        />
      )}

      <main
        id="main"
        className={cn(
          'h-screen min-w-0 overflow-x-hidden overflow-y-auto pt-topbar transition-[padding-left] duration-150 ease-out-default',
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
