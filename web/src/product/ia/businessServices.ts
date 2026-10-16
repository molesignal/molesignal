import { Waypoints } from 'lucide-react';

import type { ProductRouteMeta } from '../ia';

export const BUSINESS_SERVICE_PRODUCT_ROUTES = [
  {
    id: 'business.services',
    path: '/business-services',
    labelKey: 'business_services',
    group: 'reliability',
    icon: Waypoints,
    owner: 'business_services',
    emptyStateStrategy: 'create-first',
    nav: true,
  },
  {
    id: 'business.service.detail',
    path: '/business-services/:id',
    labelKey: 'breadcrumbs.business_service_detail',
    group: 'reliability',
    icon: Waypoints,
    owner: 'business_services',
    emptyStateStrategy: 'query-first',
    breadcrumbs: [
      { labelKey: 'business_services', to: '/business-services' },
      { labelKey: 'breadcrumbs.business_service_detail' },
    ],
  },
] as const satisfies readonly ProductRouteMeta[];
