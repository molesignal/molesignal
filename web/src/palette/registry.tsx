import {
  Activity,
  AlertTriangle,
  Bookmark,
  Clock,
  Copy,
  LayoutDashboard,
  LogOut,
  type LucideIcon,
  MapPin,
  Moon,
  Network,
  Palette,
  Settings,
  SquareCode,
} from 'lucide-react';

import type { FeatureGateStatus, FeatureKey } from '@/product/edition';
import { PRODUCT_NAV_ITEMS } from '@/product/ia';
import type { TimeWindow } from '@/stores/useTimeStore';

export type ItemKind = 'action' | 'stream' | 'service' | 'dashboard' | 'saved_view' | 'alert' | 'incident';

export interface ResultItem {
  kind: ItemKind;
  id: string;
  label: string;
  subtitle?: string | undefined;
  icon?: LucideIcon | undefined;
  shortcut?: string | undefined;
  priority?: number | undefined;
  feature?: FeatureKey | undefined;
  gateStatus?: FeatureGateStatus | undefined;
  /** Static action: handler called with the open mode. Remote items: handled by their `kind` handler. */
  run?: (ctx: { mode: OpenMode; navigate: (to: string) => void }) => void;
}

export type OpenMode = 'replace' | 'new_stack' | 'append_layer';

export interface StaticActionsApi {
  setTimeWindow: (w: TimeWindow) => void;
  toggleTheme: () => void;
  toggleDensity: () => void;
  pinAnchor: () => void;
  copyInvestigationLink: () => void;
  openHelp: () => void;
  signOut: () => void;
  /** i18n translator scoped to the `palette` namespace. */
  t: (key: string) => string;
  tNav: (key: string) => string;
  tEdition: (key: string) => string;
  currentPath?: string;
  canAccessPath?: (path: string) => boolean;
  gateStatus?: (feature: FeatureKey) => FeatureGateStatus;
}

export function buildStaticActions(api: StaticActionsApi): ResultItem[] {
  const { t } = api;
  const time = (key: string, win: TimeWindow): ResultItem => ({
    kind: 'action',
    id: `time:${key}`,
    label: t(`actions.time_${key}`),
    icon: Clock,
    run: () => api.setTimeWindow(win),
  });
  const gated = (feature: FeatureKey, item: ResultItem): ResultItem | null => {
    const status = api.gateStatus?.(feature) ?? 'allowed';
    if (status === 'permission-denied') return null;
    return {
      ...item,
      feature,
      gateStatus: status,
      subtitle: status === 'allowed' ? item.subtitle : api.tEdition(`badges.${status}`),
    };
  };
  const pathGated = (
    path: string,
    item: ResultItem | null,
  ): ResultItem | null =>
    item && (api.canAccessPath?.(path) ?? true) ? item : null;
  const routeActions: ResultItem[] = PRODUCT_NAV_ITEMS.filter(
    (route) => api.canAccessPath?.(route.path) ?? true,
  ).map((route) => ({
    kind: 'action',
    id: `route:${route.id}`,
    label: api.tNav(route.labelKey),
    icon: route.icon,
    run: ({ navigate }) => navigate(route.path),
  }));
  const creationActions: Array<ResultItem | null> = [
    pathGated('/dashboards/new/edit', {
      kind: 'action',
      id: 'create:dashboard',
      label: t('actions.create_dashboard'),
      icon: LayoutDashboard,
      priority: api.currentPath?.startsWith('/dashboards') ? -20 : -4,
      run: ({ navigate }) => navigate('/dashboards/new/edit'),
    }),
    pathGated('/alerts/rules/new', {
      kind: 'action',
      id: 'create:alert',
      label: t('actions.create_alert'),
      icon: AlertTriangle,
      priority: api.currentPath?.startsWith('/alerts') ? -20 : -3,
      run: ({ navigate }) => navigate('/alerts'),
    }),
    pathGated('/pipelines/new', {
      kind: 'action',
      id: 'create:pipeline',
      label: t('actions.create_pipeline'),
      icon: SquareCode,
      run: ({ navigate }) => navigate('/pipelines/new'),
    }),
    pathGated('/datasource', {
      kind: 'action',
      id: 'onboarding:datasource',
      label: t('actions.connect_source'),
      icon: Network,
      priority: api.currentPath === '/home' ? -30 : -2,
      run: ({ navigate }) => navigate('/datasource'),
    }),
    pathGated(
      '/account/billing',
      gated('saas-billing', {
        kind: 'action',
        id: 'account:billing',
        label: t('actions.open_billing'),
        icon: Settings,
        run: ({ navigate }) => navigate('/account/billing'),
      }),
    ),
    pathGated(
      '/account/support',
      gated('saas-support', {
        kind: 'action',
        id: 'account:support',
        label: t('actions.open_support'),
        icon: Settings,
        run: ({ navigate }) => navigate('/account/support'),
      }),
    ),
  ];
  const navigationActions = [
    pathGated('/investigate', {
      kind: 'action',
      id: 'goto:investigate',
      label: t('actions.goto_investigate'),
      icon: Activity,
      shortcut: 'mod+alt+t',
      run: ({ navigate }) => navigate('/investigate'),
    }),
    pathGated('/apm/services', {
      kind: 'action',
      id: 'goto:apm',
      label: t('actions.goto_apm'),
      icon: Network,
      shortcut: 'mod+alt+s',
      run: ({ navigate }) => navigate('/apm/services'),
    }),
    pathGated('/dashboards', {
      kind: 'action',
      id: 'goto:dashboards',
      label: t('actions.goto_dashboards'),
      icon: LayoutDashboard,
      shortcut: 'mod+alt+d',
      run: ({ navigate }) => navigate('/dashboards'),
    }),
    pathGated('/alerts/incidents', {
      kind: 'action',
      id: 'goto:incidents',
      label: t('actions.goto_incidents'),
      icon: AlertTriangle,
      shortcut: 'mod+alt+a',
      run: ({ navigate }) => navigate('/alerts/incidents'),
    }),
    pathGated('/saved-views', {
      kind: 'action',
      id: 'goto:saved-views',
      label: t('actions.goto_saved_views'),
      icon: Bookmark,
      run: ({ navigate }) => navigate('/saved-views'),
    }),
    pathGated('/investigate', {
      kind: 'action',
      id: 'action:sql',
      label: t('actions.run_sql'),
      icon: SquareCode,
      run: ({ navigate }) => navigate('/investigate?frame=sql'),
    }),
    pathGated('/investigate', {
      kind: 'action',
      id: 'action:promql',
      label: t('actions.run_promql'),
      icon: SquareCode,
      run: ({ navigate }) => navigate('/investigate?frame=promql'),
    }),
  ].filter((item): item is ResultItem => item !== null);

  return [
    ...creationActions.filter((item): item is ResultItem => item !== null),
    ...routeActions,
    ...navigationActions,
    time('last_5m', { from: 'now-5m', to: 'now', mode: 'relative' }),
    time('last_15m', { from: 'now-15m', to: 'now', mode: 'relative' }),
    time('last_1h', { from: 'now-1h', to: 'now', mode: 'relative' }),
    time('last_6h', { from: 'now-6h', to: 'now', mode: 'relative' }),
    time('last_24h', { from: 'now-24h', to: 'now', mode: 'relative' }),
    time('last_7d', { from: 'now-7d', to: 'now', mode: 'relative' }),
    {
      kind: 'action',
      id: 'action:pin-anchor',
      label: t('actions.pin_anchor'),
      icon: MapPin,
      shortcut: 'mod+alt+p',
      run: () => api.pinAnchor(),
    },
    {
      kind: 'action',
      id: 'action:copy-link',
      label: t('actions.copy_link'),
      icon: Copy,
      shortcut: 'mod+alt+y',
      run: () => api.copyInvestigationLink(),
    },
    {
      kind: 'action',
      id: 'action:toggle-theme',
      label: t('actions.toggle_theme'),
      icon: Moon,
      run: () => api.toggleTheme(),
    },
    {
      kind: 'action',
      id: 'action:toggle-density',
      label: t('actions.toggle_density'),
      icon: Palette,
      run: () => api.toggleDensity(),
    },
    {
      kind: 'action',
      id: 'action:help',
      label: t('actions.help'),
      icon: Settings,
      shortcut: 'mod+/',
      run: () => api.openHelp(),
    },
    {
      kind: 'action',
      id: 'action:sign-out',
      label: t('actions.sign_out'),
      icon: LogOut,
      run: () => api.signOut(),
    },
  ];
}

const USAGE_KEY = 'molesignal-palette-usage';

export function bumpUsage(id: string) {
  try {
    const m = JSON.parse(localStorage.getItem(USAGE_KEY) ?? '{}') as Record<string, number>;
    m[id] = Date.now();
    localStorage.setItem(USAGE_KEY, JSON.stringify(m));
  } catch {
    // ignore quota
  }
}

export function getUsage(id: string): number {
  try {
    const m = JSON.parse(localStorage.getItem(USAGE_KEY) ?? '{}') as Record<string, number>;
    return m[id] ?? 0;
  } catch {
    return 0;
  }
}

export const KIND_PRIORITY: Record<ItemKind, number> = {
  action: 0,
  incident: 1,
  saved_view: 2,
  dashboard: 3,
  stream: 4,
  service: 5,
  alert: 6,
};
