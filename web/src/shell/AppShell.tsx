import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Outlet, useLocation, useNavigate } from 'react-router-dom';

import {
  canAccessProductPath,
  useProductAccess,
} from '@/product/access';
import { findProductRoute } from '@/product/ia';
import { ProductRouteAccessGuard } from '@/routes/RouteGuard';
import { InvestigationContextBar } from '@/shell/InvestigationContextBar';
import { cn } from '@/shell/lib/cn';
import { activeNavigationEntry } from '@/shell/menu/entries';
import { MainMenu } from '@/shell/menu/MainMenu';
import { useMenuPreference } from '@/shell/menu/preference';
import { useHoverOpen } from '@/shell/menu/useHoverOpen';
import { MoleAgentPanel } from '@/shell/MoleAgentPanel';
import { useSpotlight } from '@/shell/polish/useSpotlight';
import { Topbar } from '@/shell/Topbar';
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
 * Shell layout: the fixed top bar over the page, which takes the whole width. The navigation
 * hangs from the menu button in the top bar and drops down over the page when opened. The
 * `<main>` element is offset by the top bar via padding so scrollable page bodies don't slide
 * under it. The InvestigationContextBar (when an investigation is active) and the Mole Agent
 * slide-out are mounted here at the shell level so they persist across route changes.
 */
export function AppShell(_props: AppShellProps) {
  const { t } = useTranslation('shell');
  const [menuOpen, setMenuOpen] = React.useState(false);
  const closeMenu = React.useCallback(() => setMenuOpen(false), []);
  const menuHover = useHoverOpen(menuOpen, setMenuOpen);
  const location = useLocation();
  const isSurfaceWorkbench = isSurfaceWorkbenchRoute(location.pathname);
  const nav = useNavigate();
  const { visit } = useMenuPreference();
  const area = activeNavigationEntry(location.pathname);
  const toggleMoleAgent = useMoleAgentStore((s) => s.toggle);
  const access = useProductAccess();
  const canUseMoleAgent = canAccessProductPath('/agent', access);
  useSpotlight();

  React.useEffect(() => {
    const route = area ?? findProductRoute(location.pathname);
    document.title = route ? `${t(`nav:${route.labelKey}`)}｜MoleSignal` : 'MoleSignal';
    return () => { document.title = 'MoleSignal'; };
  }, [area, location.pathname, t]);

  React.useEffect(() => {
    setMenuOpen(false);
  }, [location.pathname]);

  // The area opened goes to the head of the menu's recent list (once the list has been read).
  React.useEffect(() => {
    if (area) visit(area.id);
  }, [area, visit]);

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

  return (
    <div
      data-shell-layout={isSurfaceWorkbench ? 'surface-workbench' : undefined}
      className={cn(
        // `h-dvh` follows the visible viewport on mobile browsers whose address
        // bar collapses; `h-screen` (100vh) would hide the page's last rows
        // behind it.
        'h-dvh min-w-0 overflow-hidden bg-bg-0 text-tx-0',
        isSurfaceWorkbench && 'bg-[var(--page-canvas)]',
      )}
    >
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:left-2 focus:top-2 focus:z-[100] focus:rounded focus:bg-indigo focus:px-2 focus:py-1 focus:text-white"
      >
        Skip to content
      </a>

      <Topbar
        menuOpen={menuOpen}
        menuHover={menuHover}
        onPaletteOpen={_props.onPaletteOpen}
        onNocOpen={() => nav('/noc')}
        surfaceWorkbench={isSurfaceWorkbench}
      />

      <MainMenu open={menuOpen} onClose={closeMenu} hover={menuHover} />

      <main
        id="main"
        className="h-dvh min-w-0 overflow-x-hidden overflow-y-auto pt-topbar"
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
