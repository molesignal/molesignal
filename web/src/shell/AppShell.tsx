import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Outlet, useLocation, useNavigate } from 'react-router-dom';

import {
  canAccessProductPath,
  canAccessProductRoute,
  useProductAccess,
} from '@/product/access';
import { findProductRoute } from '@/product/ia';
import { ProductRouteAccessGuard } from '@/routes/RouteGuard';
import { InvestigationContextBar } from '@/shell/InvestigationContextBar';
import { cn } from '@/shell/lib/cn';
import { MoleAgentPanel } from '@/shell/MoleAgentPanel';
import { Sidebar } from '@/shell/Sidebar';
import { Topbar } from '@/shell/Topbar';
import { UnsupportedScreen } from '@/shell/UnsupportedScreen';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';
import { useSidebarStore } from '@/stores/useSidebarStore';

interface AppShellProps {
  onTimePickerOpen: () => void;
  onPaletteOpen: () => void;
}

/** Below this the dense SRE layout has no fallback — see UnsupportedScreen. */
const DESKTOP_MIN_WIDTH = 1024;

function useViewportWidth(): number {
  const [width, setWidth] = React.useState(() =>
    typeof window === 'undefined' ? DESKTOP_MIN_WIDTH : window.innerWidth,
  );
  React.useEffect(() => {
    const onResize = () => setWidth(window.innerWidth);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);
  return width;
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
  const [collapsed, setCollapsed] = React.useState(false);
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
  const primarySidebarCollapsed = isAutoCollapsedRoute
    ? !autoCollapsedSidebarExpanded
    : collapsed;
  const nav = useNavigate();
  const viewportWidth = useViewportWidth();
  const recordVisit = useSidebarStore((s) => s.recordVisit);
  const toggleMoleAgent = useMoleAgentStore((s) => s.toggle);
  const access = useProductAccess();
  const canUseMoleAgent = canAccessProductPath('/intelligence', access);

  React.useEffect(() => {
    setMobileNavOpen(false);
  }, [location.pathname]);

  React.useEffect(() => {
    if (!isAutoCollapsedRoute) setAutoCollapsedSidebarExpanded(false);
  }, [isAutoCollapsedRoute]);

  // Feed the sidebar "Recent" list with real destinations that DON'T already
  // have a permanent home in a fixed nav group — nav items live in their group,
  // so Recent surfaces the rest (e.g. saved views, service graph). Skip
  // parameterized detail routes (`/x/:id`) so deep pages don't crowd the list,
  // and skip nav items so Recent never just echoes a fixed group.
  React.useEffect(() => {
    const route = findProductRoute(location.pathname);
    if (
      route &&
      canAccessProductRoute(route, access) &&
      !route.nav &&
      !route.path.includes(':')
    ) {
      recordVisit(route.id);
    }
  }, [access, location.pathname, recordVisit]);

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
    if (isAutoCollapsedRoute) {
      setAutoCollapsedSidebarExpanded((v) => !v);
      return;
    }
    setCollapsed((v) => !v);
  };

  // Dense investigation surfaces remain desktop-only. Management routes have
  // their own narrow-screen navigation drawers and responsive content, so they
  // can bypass the interstitial without claiming mobile support for the whole
  // console.
  // Hooks above run unconditionally so this early return stays hook-safe.
  if (viewportWidth < DESKTOP_MIN_WIDTH && !isManagementRoute) {
    return <UnsupportedScreen width={viewportWidth} />;
  }

  return (
    <div className="h-screen min-w-0 overflow-hidden bg-bg-0 text-tx-0">
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
        collapsed={primarySidebarCollapsed}
        mobileOpen={mobileNavOpen}
        onNavigate={() => setMobileNavOpen(false)}
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
