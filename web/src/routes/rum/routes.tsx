import { Navigate, type RouteObject, useLocation } from 'react-router-dom';

import { FirstOpenablePage } from '@/product/moduleAccess';
import { QueryTabs } from '@/shell/queryTabs';

import {
  Applications,
  RumAnalytics,
  RumAnalyticsHeatmaps,
  RumAnalyticsPaths,
  RumAnalyticsRetention,
  RumErrors,
  RumExplorer,
  RumExplorerFrame,
  RumFeatureFlags,
  RumFrustration,
  RumOverview,
  RumPerformance,
  RumPerformanceLongTasks,
  RumPerformanceResources,
  RumPlaylistPage,
  RumPlaylists,
  RumSettingsGuide,
  SessionDetail,
  SourceMaps,
  UploadSourceMaps,
} from './index';
import { RUM_PAGES } from './RumLayout';
import {
  legacyApmUserExperienceTarget,
  legacyRumSettingsTarget,
} from '../apm/compat';

export const RUM_ROUTE_CHILDREN: RouteObject[] = [
  { index: true, element: <FirstOpenablePage paths={RUM_PAGES} /> },
  { path: 'overview', element: <RumOverview /> },
  {
    path: 'explorer',
    element: (
      <RumExplorerFrame>
        <QueryTabs kind="rum">
          <RumExplorer />
        </QueryTabs>
      </RumExplorerFrame>
    ),
  },
  { path: 'errors', element: <RumErrors /> },
  { path: 'performance', element: <RumPerformance /> },
  { path: 'performance/resources', element: <RumPerformanceResources /> },
  { path: 'performance/long-tasks', element: <RumPerformanceLongTasks /> },
  { path: 'frustration', element: <RumFrustration /> },
  { path: 'analytics', element: <RumAnalytics /> },
  { path: 'analytics/paths', element: <RumAnalyticsPaths /> },
  { path: 'analytics/retention', element: <RumAnalyticsRetention /> },
  { path: 'analytics/heatmaps', element: <RumAnalyticsHeatmaps /> },
  { path: 'feature-flags', element: <RumFeatureFlags /> },
  { path: 'playlists', element: <RumPlaylists /> },
  { path: 'playlists/:id', element: <RumPlaylistPage /> },
  { path: 'applications', element: <Applications /> },
  { path: 'sessions/view/:id', element: <SessionDetail /> },
  {
    path: 'settings',
    element: <Navigate to="/rum/settings/sdk" replace />,
  },
  { path: 'settings/source-maps', element: <SourceMaps /> },
  { path: 'settings/source-maps/upload', element: <UploadSourceMaps /> },
  { path: 'settings/:section', element: <RumSettingsGuide /> },
  { path: 'source-maps', element: <LegacyRumSettingsRedirect /> },
  { path: 'upload-source-maps', element: <LegacyRumSettingsRedirect /> },
];

export function LegacyApmUserExperienceRedirect() {
  const location = useLocation();
  return (
    <Navigate
      to={legacyApmUserExperienceTarget(
        location.pathname,
        location.search,
        location.hash,
      )}
      replace
    />
  );
}

function LegacyRumSettingsRedirect() {
  const location = useLocation();
  return (
    <Navigate
      to={legacyRumSettingsTarget(
        location.pathname,
        location.search,
        location.hash,
      )}
      replace
    />
  );
}
