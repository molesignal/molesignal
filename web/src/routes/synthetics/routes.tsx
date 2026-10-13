import type { RouteObject } from 'react-router-dom';
import { Navigate, useParams } from 'react-router-dom';

import { Agents } from './agents/Agents';
import { CheckDetail } from './checks/detail/index';
import { Checks } from './checks/index';
import { Locations } from './locations/index';
import { SyntheticsOverview } from './overview/index';
import { Results } from './results/index';
import { Variables } from './variables/index';

export const SYNTHETICS_ROUTES: RouteObject[] = [
  { path: 'synthetics', element: <Navigate to="/synthetics/overview" replace /> },
  { path: 'synthetics/overview', element: <SyntheticsOverview /> },
  { path: 'synthetics/checks', element: <Checks /> },
  { path: 'synthetics/checks/new', element: <Checks /> },
  { path: 'synthetics/checks/:monitorId/edit', element: <Checks /> },
  { path: 'synthetics/checks/:monitorId', element: <CheckDetail /> },
  { path: 'synthetics/checks/:monitorId/results', element: <CheckDetail /> },
  { path: 'synthetics/checks/:monitorId/results/:resultId', element: <CheckDetail /> },
  { path: 'synthetics/checks/:monitorId/configuration', element: <CheckDetail /> },
  { path: 'synthetics/checks/:monitorId/revisions', element: <CheckDetail /> },
  { path: 'synthetics/locations', element: <Locations /> },
  { path: 'synthetics/locations/:locationId', element: <Locations /> },
  { path: 'synthetics/agents', element: <Agents /> },
  { path: 'synthetics/agents/:agentId', element: <Agents /> },
  { path: 'synthetics/results', element: <Results /> },
  { path: 'synthetics/results/:resultId', element: <Results /> },
  { path: 'synthetics/variables', element: <Variables /> },

  // Addresses that were tabs of their own. The kinds of check are views of the
  // one list of checks, a schedule is a column of it, and assertion templates
  // are where a new check starts.
  { path: 'synthetics/api-tests', element: <Navigate to="/synthetics/checks?kind=api" replace /> },
  {
    path: 'synthetics/browser-tests',
    element: <Navigate to="/synthetics/checks?kind=browser" replace />,
  },
  { path: 'synthetics/network', element: <Navigate to="/synthetics/checks?kind=network" replace /> },
  { path: 'synthetics/schedules', element: <Navigate to="/synthetics/checks" replace /> },
  { path: 'synthetics/assertions', element: <Navigate to="/synthetics/checks/new" replace /> },
  { path: 'synthetics/settings', element: <Navigate to="/synthetics/overview" replace /> },
  { path: 'synthetics/monitors', element: <Navigate to="/synthetics/checks" replace /> },
  {
    path: 'synthetics/monitors/:monitorId',
    element: <LegacyMonitorRedirect />,
  },
];

function LegacyMonitorRedirect() {
  const { monitorId } = useParams();
  return <Navigate to={`/synthetics/checks/${monitorId ?? ''}`} replace />;
}
