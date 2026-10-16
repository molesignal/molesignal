import type { RouteObject } from 'react-router-dom';

import { BusinessServiceDetail } from './detail';
import { BusinessServices } from './list';

export const BUSINESS_SERVICE_ROUTES: RouteObject[] = [
  { path: 'business-services', element: <BusinessServices /> },
  { path: 'business-services/:serviceId', element: <BusinessServiceDetail /> },
];
