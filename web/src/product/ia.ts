import {
  Bot,
  Bookmark,
  ChartSpline,
  Compass,
  FileBarChart,
  Flame,
  GitBranch,
  Home as HomeIcon,
  LayoutDashboard,
  type LucideIcon,
  ScrollText,
  Settings,
  Share2,
  ShieldCheck,
  Workflow,
} from 'lucide-react';

import { ALERT_PRODUCT_ROUTES } from './ia/alerts';
import { APM_PRODUCT_ROUTES } from './ia/apm';
import { DATA_COLLABORATION_PRODUCT_ROUTES } from './ia/dataCollaboration';
import { RUM_PRODUCT_ROUTES } from './ia/rum';

export const PRODUCT_NAV_GROUPS = [
  'home',
  'investigate',
  'data',
  'collaboration',
  'admin',
] as const;

export type ProductNavGroup = (typeof PRODUCT_NAV_GROUPS)[number];
export type ProductEdition = 'any' | 'oss' | 'pro' | 'saas';
export type ProductEmptyStateStrategy =
  | 'none'
  | 'activation'
  | 'query-first'
  | 'create-first'
  | 'backend-pending'
  | 'license-gated'
  | 'permission-denied';

export type ProductOwnerModule =
  | 'home'
  | 'datasource'
  | 'logs'
  | 'metrics'
  | 'traces'
  | 'apm'
  | 'rum'
  | 'profiles'
  | 'dashboards'
  | 'alerts'
  | 'streams'
  | 'pipelines'
  | 'functions'
  | 'reports'
  | 'status_pages'
  | 'saved_views'
  | 'automation'
  | 'iam'
  | 'settings'
  | 'account'
  | 'agent'
  | 'legacy';

export interface ProductNavGroupMeta {
  labelKey: string;
  icon: LucideIcon;
}

export interface ProductBreadcrumbItem {
  labelKey: string;
  label?: string;
  to?: string;
}

export interface ProductRouteMeta {
  id: string;
  path: `/${string}`;
  labelKey: string;
  group: ProductNavGroup;
  icon: LucideIcon;
  edition: ProductEdition;
  owner: ProductOwnerModule;
  emptyStateStrategy: ProductEmptyStateStrategy;
  nav?: boolean;
  exact?: boolean;
  breadcrumbs?: readonly ProductBreadcrumbItem[];
  backTo?: string;
}

type ProductRouteInput = Omit<ProductRouteMeta, 'edition'> &
  Partial<Pick<ProductRouteMeta, 'edition'>>;

export const PRODUCT_NAV_GROUP_META = {
  home: { labelKey: 'groups.home', icon: HomeIcon },
  investigate: { labelKey: 'groups.investigate', icon: ScrollText },
  data: { labelKey: 'groups.data', icon: Workflow },
  collaboration: { labelKey: 'groups.collaboration', icon: FileBarChart },
  admin: { labelKey: 'groups.admin', icon: ShieldCheck },
} satisfies Record<ProductNavGroup, ProductNavGroupMeta>;

export const PRODUCT_ROUTES = [
  route({
    id: 'home',
    path: '/home',
    labelKey: 'home',
    group: 'home',
    icon: HomeIcon,
    owner: 'home',
    emptyStateStrategy: 'activation',
    nav: true,
    exact: true,
  }),
  route({
    id: 'dashboards',
    path: '/dashboards',
    labelKey: 'dashboards',
    group: 'investigate',
    icon: LayoutDashboard,
    owner: 'dashboards',
    emptyStateStrategy: 'create-first',
    nav: true,
  }),
  route({
    id: 'metrics',
    path: '/metrics',
    labelKey: 'metrics',
    group: 'investigate',
    icon: ChartSpline,
    owner: 'metrics',
    emptyStateStrategy: 'query-first',
    nav: true,
  }),
  route({
    id: 'logs',
    path: '/logs',
    labelKey: 'logs',
    group: 'investigate',
    icon: ScrollText,
    owner: 'logs',
    emptyStateStrategy: 'query-first',
    nav: true,
  }),
  route({
    id: 'logs.inspector',
    path: '/logs/inspector',
    labelKey: 'breadcrumbs.logs_inspector',
    group: 'investigate',
    icon: ScrollText,
    owner: 'logs',
    emptyStateStrategy: 'query-first',
    breadcrumbs: [crumb('logs', '/logs'), crumb('breadcrumbs.logs_inspector')],
    backTo: '/logs',
  }),
  route({
    id: 'traces',
    path: '/traces',
    labelKey: 'traces',
    group: 'investigate',
    icon: Share2,
    owner: 'traces',
    emptyStateStrategy: 'query-first',
    nav: true,
  }),
  route({
    id: 'trace.session.detail',
    path: '/traces/session/:id',
    labelKey: 'breadcrumbs.trace_session',
    group: 'investigate',
    icon: Share2,
    owner: 'traces',
    emptyStateStrategy: 'query-first',
    breadcrumbs: [crumb('traces', '/traces'), crumb('breadcrumbs.trace_session')],
    backTo: '/traces',
  }),
  route({
    id: 'trace.detail',
    path: '/traces/:id',
    labelKey: 'breadcrumbs.trace_detail',
    group: 'investigate',
    icon: Share2,
    owner: 'traces',
    emptyStateStrategy: 'query-first',
    breadcrumbs: [crumb('traces', '/traces'), crumb('breadcrumbs.trace_detail')],
    backTo: '/traces',
  }),
  route({
    id: 'service.graph',
    path: '/service-graph',
    labelKey: 'service_graph',
    group: 'investigate',
    icon: GitBranch,
    owner: 'traces',
    emptyStateStrategy: 'query-first',
  }),
  ...APM_PRODUCT_ROUTES,
  ...RUM_PRODUCT_ROUTES,
  route({
    id: 'profiles',
    path: '/profiles',
    labelKey: 'profiles',
    group: 'investigate',
    icon: Flame,
    owner: 'profiles',
    emptyStateStrategy: 'activation',
    nav: true,
  }),
  route({
    id: 'profiles.compare',
    path: '/profiles/compare',
    labelKey: 'breadcrumbs.profiles_compare',
    group: 'investigate',
    icon: Flame,
    owner: 'profiles',
    emptyStateStrategy: 'query-first',
    breadcrumbs: [crumb('profiles', '/profiles'), crumb('breadcrumbs.profiles_compare')],
    backTo: '/profiles',
  }),
  route({
    id: 'profile.detail',
    path: '/profiles/:id',
    labelKey: 'breadcrumbs.profile_detail',
    group: 'investigate',
    icon: Flame,
    owner: 'profiles',
    emptyStateStrategy: 'query-first',
    breadcrumbs: [crumb('profiles', '/profiles'), crumb('breadcrumbs.profile_detail')],
    backTo: '/profiles',
  }),
  route({
    id: 'dashboard.new.edit',
    path: '/dashboards/new/edit',
    labelKey: 'breadcrumbs.dashboard_new',
    group: 'investigate',
    icon: LayoutDashboard,
    owner: 'dashboards',
    emptyStateStrategy: 'create-first',
    breadcrumbs: [crumb('dashboards', '/dashboards'), crumb('breadcrumbs.dashboard_new')],
    backTo: '/dashboards',
  }),
  route({
    id: 'dashboard.import',
    path: '/dashboards/import',
    labelKey: 'breadcrumbs.dashboard_import',
    group: 'investigate',
    icon: LayoutDashboard,
    owner: 'dashboards',
    emptyStateStrategy: 'create-first',
    breadcrumbs: [crumb('dashboards', '/dashboards'), crumb('breadcrumbs.dashboard_import')],
    backTo: '/dashboards',
  }),
  route({
    id: 'dashboard.detail',
    path: '/dashboards/:id',
    labelKey: 'breadcrumbs.dashboard_detail',
    group: 'investigate',
    icon: LayoutDashboard,
    owner: 'dashboards',
    emptyStateStrategy: 'create-first',
    breadcrumbs: [crumb('dashboards', '/dashboards'), crumb('breadcrumbs.dashboard_detail')],
    backTo: '/dashboards',
  }),
  route({
    id: 'dashboard.edit',
    path: '/dashboards/:id/edit',
    labelKey: 'breadcrumbs.dashboard_edit',
    group: 'investigate',
    icon: LayoutDashboard,
    owner: 'dashboards',
    emptyStateStrategy: 'create-first',
    breadcrumbs: [crumb('dashboards', '/dashboards'), crumb('breadcrumbs.dashboard_detail'), crumb('breadcrumbs.dashboard_edit')],
    backTo: '/dashboards',
  }),
  route({
    id: 'dashboard.new.panel',
    path: '/dashboards/:id/panels/new',
    labelKey: 'breadcrumbs.dashboard_new_panel',
    group: 'investigate',
    icon: LayoutDashboard,
    owner: 'dashboards',
    emptyStateStrategy: 'create-first',
    breadcrumbs: [crumb('dashboards', '/dashboards'), crumb('breadcrumbs.dashboard_detail'), crumb('breadcrumbs.dashboard_new_panel')],
    backTo: '/dashboards',
  }),
  ...ALERT_PRODUCT_ROUTES,
  route({
    id: 'agent',
    path: '/agent',
    labelKey: 'agent',
    group: 'investigate',
    icon: Bot,
    owner: 'agent',
    emptyStateStrategy: 'none',
    nav: true,
  }),
  route({
    id: 'saved.views',
    path: '/saved-views',
    labelKey: 'saved_views',
    group: 'investigate',
    icon: Bookmark,
    owner: 'saved_views',
    emptyStateStrategy: 'create-first',
  }),
  ...DATA_COLLABORATION_PRODUCT_ROUTES,
  route({
    id: 'iam.users',
    path: '/iam/users',
    labelKey: 'iam',
    group: 'admin',
    icon: ShieldCheck,
    owner: 'iam',
    emptyStateStrategy: 'create-first',
    nav: true,
  }),
  route({
    id: 'iam.organizations',
    path: '/iam/organizations',
    labelKey: 'iam',
    group: 'admin',
    icon: ShieldCheck,
    owner: 'iam',
    emptyStateStrategy: 'create-first',
    nav: true,
  }),
  route({
    id: 'settings.general',
    path: '/settings/general',
    labelKey: 'settings',
    group: 'admin',
    icon: Settings,
    owner: 'settings',
    emptyStateStrategy: 'none',
    nav: true,
  }),
  route({
    id: 'settings.organization.management',
    path: '/settings/organization_management',
    labelKey: 'settings',
    group: 'admin',
    icon: Settings,
    owner: 'settings',
    emptyStateStrategy: 'none',
    nav: true,
  }),
  route({
    id: 'settings.license',
    path: '/settings/license',
    labelKey: 'breadcrumbs.license',
    group: 'admin',
    icon: ShieldCheck,
    owner: 'settings',
    emptyStateStrategy: 'none',
    nav: true,
    breadcrumbs: [crumb('settings', '/settings/general'), crumb('breadcrumbs.license')],
  }),
  route({
    id: 'settings.client_ip',
    path: '/settings/client_ip',
    labelKey: 'client_ip',
    group: 'admin',
    icon: ShieldCheck,
    owner: 'settings',
    emptyStateStrategy: 'none',
  }),
  route({
    id: 'account.billing',
    path: '/account/billing',
    labelKey: 'account_billing',
    group: 'admin',
    icon: FileBarChart,
    edition: 'saas',
    owner: 'account',
    emptyStateStrategy: 'license-gated',
    breadcrumbs: [crumb('account_billing')],
  }),
  route({
    id: 'account.support',
    path: '/account/support',
    labelKey: 'account_support',
    group: 'admin',
    icon: ShieldCheck,
    edition: 'saas',
    owner: 'account',
    emptyStateStrategy: 'license-gated',
    breadcrumbs: [crumb('account_support')],
  }),
  route({
    id: 'investigate',
    path: '/investigate',
    labelKey: 'investigate_stack',
    group: 'investigate',
    icon: Compass,
    owner: 'legacy',
    emptyStateStrategy: 'query-first',
  }),
] as const satisfies readonly ProductRouteMeta[];

export type ProductRouteId = (typeof PRODUCT_ROUTES)[number]['id'];
export type ProductNavRoute = ProductRouteMeta & { nav: true };

export const PRODUCT_ROUTE_BY_ID = Object.fromEntries(
  PRODUCT_ROUTES.map((routeMeta) => [routeMeta.id, routeMeta]),
) as unknown as Record<ProductRouteId, ProductRouteMeta>;

export const PRODUCT_HOME_ROUTE = requireProductRoute('home');

export const PRODUCT_NAV_ITEMS = (PRODUCT_ROUTES as readonly ProductRouteMeta[]).filter(
  (routeMeta): routeMeta is ProductNavRoute => isProductNavRoute(routeMeta),
);

export function getProductNavItems(group: ProductNavGroup): ProductNavRoute[] {
  return PRODUCT_NAV_ITEMS.filter((routeMeta) => routeMeta.group === group);
}

export function getProductRouteById(id: string): ProductRouteMeta | undefined {
  return PRODUCT_ROUTE_BY_ID[id as ProductRouteId];
}

export function findProductRoute(pathname: string): ProductRouteMeta | undefined {
  const normalizedPathname = normalizePathname(pathname);
  return (
    PRODUCT_ROUTES.find((routeMeta) => routeMeta.path === normalizedPathname) ??
    PRODUCT_ROUTES.find((routeMeta) => routeMatchesPath(routeMeta.path, normalizedPathname))
  );
}

function route<const T extends ProductRouteInput>(
  routeMeta: T,
): T & { edition: ProductEdition } {
  return {
    edition: 'any',
    ...routeMeta,
  } as T & { edition: ProductEdition };
}

function crumb(labelKey: string, to?: string): ProductBreadcrumbItem {
  return to ? { labelKey, to } : { labelKey };
}

function requireProductRoute(id: ProductRouteId): ProductRouteMeta {
  const routeMeta = PRODUCT_ROUTE_BY_ID[id];
  if (!routeMeta) throw new Error(`Missing product route: ${id}`);
  return routeMeta;
}

function isProductNavRoute(routeMeta: ProductRouteMeta): routeMeta is ProductNavRoute {
  return routeMeta.nav === true;
}

function normalizePathname(pathname: string): string {
  const [pathOnly = '/'] = pathname.split('?');
  const withoutHash = pathOnly.split('#')[0] ?? '/';
  const withoutTrailingSlash = withoutHash.length > 1 ? withoutHash.replace(/\/+$/, '') : withoutHash;
  return withoutTrailingSlash || '/';
}

function routeMatchesPath(pattern: string, pathname: string): boolean {
  if (!pattern.includes(':')) return false;
  const patternParts = pattern.split('/').filter(Boolean);
  const pathnameParts = pathname.split('/').filter(Boolean);
  if (patternParts.length !== pathnameParts.length) return false;

  return patternParts.every((part, index) => part.startsWith(':') || part === pathnameParts[index]);
}
