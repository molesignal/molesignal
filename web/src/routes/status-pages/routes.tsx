import { Navigate, type RouteObject } from 'react-router-dom';

import { StatusPages } from '.';
import { StatusPageComponents } from './workspace/Components';
import { StatusPageEvents } from './workspace/EventsPage';
import { StatusPageHistory } from './workspace/History';
import { StatusPageWorkspaceLayout } from './workspace/Layout';
import { StatusPageOverview } from './workspace/Overview';
import { StatusPageBrandingSettings } from './workspace/settings/Branding';
import { StatusPageDomainAccessSettings } from './workspace/settings/DomainAccess';
import { StatusPageGeneralSettings } from './workspace/settings/General';
import { StatusPageSettingsLayout } from './workspace/settings/Layout';
import { StatusPageLocalizationSettings } from './workspace/settings/Localization';
import { StatusPageSubscribers } from './workspace/Subscribers';

export const STATUS_PAGE_MANAGEMENT_ROUTES: RouteObject[] = [
  { path: 'status-pages', element: <StatusPages /> },
  {
    path: 'status-pages/:pageId',
    element: <StatusPageWorkspaceLayout />,
    children: [
      { index: true, element: <Navigate to="overview" replace /> },
      { path: 'overview', element: <StatusPageOverview /> },
      { path: 'components', element: <StatusPageComponents /> },
      { path: 'components/:componentId', element: <StatusPageComponents /> },
      { path: 'incidents', element: <StatusPageEvents kind="incident" /> },
      { path: 'incidents/:eventId', element: <StatusPageEvents kind="incident" /> },
      { path: 'maintenance', element: <StatusPageEvents kind="maintenance" /> },
      { path: 'maintenance/:eventId', element: <StatusPageEvents kind="maintenance" /> },
      { path: 'history', element: <StatusPageHistory /> },
      { path: 'history/:eventId', element: <StatusPageHistory /> },
      { path: 'subscribers', element: <Navigate to="list" replace /> },
      { path: 'subscribers/list', element: <StatusPageSubscribers view="list" /> },
      { path: 'subscribers/deliveries', element: <StatusPageSubscribers view="deliveries" /> },
      {
        path: 'settings',
        element: <StatusPageSettingsLayout />,
        children: [
          { index: true, element: <Navigate to="general" replace /> },
          { path: 'general', element: <StatusPageGeneralSettings /> },
          { path: 'branding', element: <StatusPageBrandingSettings /> },
          { path: 'localization', element: <StatusPageLocalizationSettings /> },
          { path: 'domain-access', element: <StatusPageDomainAccessSettings /> },
        ],
      },
    ],
  },
];
