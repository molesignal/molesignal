import { RadioTower } from 'lucide-react';

import type { ProductRouteMeta } from '../ia';

export const STATUS_PAGE_PRODUCT_ROUTES = [
  {
    id: 'status.pages',
    path: '/status-pages',
    labelKey: 'status_pages',
    group: 'reliability',
    icon: RadioTower,
    edition: 'any',
    owner: 'status_pages',
    emptyStateStrategy: 'create-first',
    nav: true,
  },
] as const satisfies readonly ProductRouteMeta[];
