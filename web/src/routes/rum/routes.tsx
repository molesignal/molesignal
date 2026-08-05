import { Navigate, type RouteObject, useLocation } from 'react-router-dom';

import {
  Applications,
  Errors,
  ErrorDetail,
  Overview,
  Pages,
  PerfApis,
  PerfErrors,
  PerfOverview,
  PerfWebVitals,
  RumSettingsGuide,
  SessionDetail,
  SessionReplay,
  Sessions,
  SourceMaps,
  UploadSourceMaps,
} from './index';
import {
  legacyApmUserExperienceTarget,
  legacyRumSettingsTarget,
} from '../apm/compat';

export const RUM_ROUTE_CHILDREN: RouteObject[] = [
  { index: true, element: <Navigate to="/rum/overview" replace /> },
  { path: 'overview', element: <Overview /> },
  { path: 'applications', element: <Applications /> },
  { path: 'sessions', element: <Sessions /> },
  { path: 'sessions/view/:id', element: <SessionDetail /> },
  { path: 'pages', element: <Pages /> },
  { path: 'errors', element: <Errors /> },
  { path: 'errors/view/:id', element: <ErrorDetail /> },
  {
    path: 'performance',
    element: <Navigate to="/rum/performance/overview" replace />,
  },
  { path: 'performance/overview', element: <PerfOverview /> },
  { path: 'performance/web-vitals', element: <PerfWebVitals /> },
  { path: 'performance/errors', element: <PerfErrors /> },
  { path: 'performance/apis', element: <PerfApis /> },
  { path: 'session-replay', element: <SessionReplay /> },
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
