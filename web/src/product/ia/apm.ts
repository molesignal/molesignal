import { Activity, Boxes, Bug, Gauge, Network, Settings2, Target } from 'lucide-react';

import type { ProductRouteMeta } from '../ia';

const BACKEND_ACCESS = {
  owner: 'apm',
  emptyStateStrategy: 'query-first',
  group: 'observe',
} as const;

export const APM_PRODUCT_ROUTES = [
  {
    ...BACKEND_ACCESS,
    id: 'apm',
    path: '/apm',
    labelKey: 'apm',
    icon: Activity,
    nav: true,
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.overview',
    path: '/apm/overview',
    labelKey: 'breadcrumbs.apm_overview',
    icon: Gauge,
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.services',
    path: '/apm/services',
    labelKey: 'breadcrumbs.apm_services',
    icon: Boxes,
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.map',
    path: '/apm/map',
    labelKey: 'breadcrumbs.apm_map',
    icon: Network,
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.errors',
    path: '/apm/errors',
    labelKey: 'breadcrumbs.apm_errors',
    icon: Bug,
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.slos',
    path: '/apm/slos',
    labelKey: 'breadcrumbs.apm_slos',
    icon: Target,
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.settings',
    path: '/apm/settings',
    labelKey: 'breadcrumbs.apm_settings',
    icon: Settings2,
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.service.detail',
    path: '/apm/services/:service',
    labelKey: 'breadcrumbs.apm_service_detail',
    icon: Activity,
    backTo: '/apm/services',
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.service.section',
    path: '/apm/services/:service/:section',
    labelKey: 'breadcrumbs.apm_service_detail',
    icon: Activity,
    backTo: '/apm/services',
  },
] as const satisfies readonly ProductRouteMeta[];
