import {
  Activity,
  Boxes,
  Bug,
  Cpu,
  Gauge,
  GitBranch,
  Network,
} from 'lucide-react';

import type { ProductRouteMeta } from '../ia';

const BACKEND_ACCESS = {
  edition: 'any',
  owner: 'apm',
  emptyStateStrategy: 'query-first',
  group: 'investigate',
} as const;

const apmCrumb = { labelKey: 'apm', to: '/apm/overview' } as const;

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
    breadcrumbs: [apmCrumb, { labelKey: 'breadcrumbs.apm_overview' }],
    backTo: '/apm',
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.services',
    path: '/apm/services',
    labelKey: 'breadcrumbs.apm_services',
    icon: Activity,
    breadcrumbs: [apmCrumb, { labelKey: 'breadcrumbs.apm_services' }],
    backTo: '/apm/overview',
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.service.detail',
    path: '/apm/services/:service',
    labelKey: 'breadcrumbs.apm_service_detail',
    icon: Activity,
    breadcrumbs: [
      apmCrumb,
      { labelKey: 'breadcrumbs.apm_services', to: '/apm/services' },
      { labelKey: 'breadcrumbs.apm_service_detail' },
    ],
    backTo: '/apm/services',
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.service.runtime',
    path: '/apm/services/:service/runtime',
    labelKey: 'breadcrumbs.apm_service_runtime',
    icon: Cpu,
    breadcrumbs: [
      apmCrumb,
      { labelKey: 'breadcrumbs.apm_services', to: '/apm/services' },
      { labelKey: 'breadcrumbs.apm_service_detail' },
      { labelKey: 'breadcrumbs.apm_service_runtime' },
    ],
    backTo: '/apm/services',
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.transactions',
    path: '/apm/transactions',
    labelKey: 'breadcrumbs.apm_transactions',
    icon: Boxes,
    breadcrumbs: [apmCrumb, { labelKey: 'breadcrumbs.apm_transactions' }],
    backTo: '/apm/overview',
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.transaction.detail',
    path: '/apm/transactions/:transaction',
    labelKey: 'breadcrumbs.apm_transaction_detail',
    icon: Boxes,
    breadcrumbs: [
      apmCrumb,
      { labelKey: 'breadcrumbs.apm_transactions', to: '/apm/transactions' },
      { labelKey: 'breadcrumbs.apm_transaction_detail' },
    ],
    backTo: '/apm/transactions',
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.dependencies',
    path: '/apm/dependencies',
    labelKey: 'breadcrumbs.apm_dependencies',
    icon: Network,
    breadcrumbs: [apmCrumb, { labelKey: 'breadcrumbs.apm_dependencies' }],
    backTo: '/apm/overview',
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.errors',
    path: '/apm/errors',
    labelKey: 'breadcrumbs.apm_errors',
    icon: Bug,
    breadcrumbs: [apmCrumb, { labelKey: 'breadcrumbs.apm_errors' }],
    backTo: '/apm/overview',
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.error.detail',
    path: '/apm/errors/:fingerprint',
    labelKey: 'breadcrumbs.apm_error_detail',
    icon: Bug,
    breadcrumbs: [
      apmCrumb,
      { labelKey: 'breadcrumbs.apm_errors', to: '/apm/errors' },
      { labelKey: 'breadcrumbs.apm_error_detail' },
    ],
    backTo: '/apm/errors',
  },
  {
    ...BACKEND_ACCESS,
    id: 'apm.deployments',
    path: '/apm/deployments',
    labelKey: 'breadcrumbs.apm_deployments',
    icon: GitBranch,
    breadcrumbs: [apmCrumb, { labelKey: 'breadcrumbs.apm_deployments' }],
    backTo: '/apm/overview',
  },
] as const satisfies readonly ProductRouteMeta[];
