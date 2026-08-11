import { Radar } from 'lucide-react';

import type { ProductRouteMeta } from '../ia';

export const SYNTHETICS_PRODUCT_ROUTES = [
  {
    id: 'synthetics',
    path: '/synthetics',
    labelKey: 'synthetics',
    group: 'reliability',
    icon: Radar,
    edition: 'any',
    owner: 'synthetics',
    emptyStateStrategy: 'create-first',
    nav: true,
  },
] as const satisfies readonly ProductRouteMeta[];
