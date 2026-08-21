import type { RouteObject } from 'react-router-dom';
import { Navigate, useParams } from 'react-router-dom';

import { Agents } from './agents/Agents';
import { CheckDetail } from './CheckDetail';
import { Checks } from './Checks';
import {
  AssertionsLibrary,
  Schedules,
  SyntheticsSettings,
} from './LibraryPages';
import { Locations } from './Locations';
import { SyntheticsOverview } from './Overview';
import { Results } from './Results';
import { Variables } from './Variables';

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
  {
    path: 'synthetics/browser-tests',
    element: (
      <Checks
        kinds={['browser']}
        titleKey="checks.browser_title"
        subtitleKey="checks.browser_subtitle"
      />
    ),
  },
  {
    path: 'synthetics/api-tests',
    element: (
      <Checks
        kinds={['http']}
        titleKey="checks.api_title"
        subtitleKey="checks.api_subtitle"
      />
    ),
  },
  {
    path: 'synthetics/network',
    element: (
      <Checks
        kinds={['tcp', 'ssh', 'dns', 'icmp', 'tls', 'grpc']}
        titleKey="checks.network_title"
        subtitleKey="checks.network_subtitle"
      />
    ),
  },
  { path: 'synthetics/schedules', element: <Schedules /> },
  { path: 'synthetics/locations', element: <Locations /> },
  { path: 'synthetics/locations/:locationId', element: <Locations /> },
  { path: 'synthetics/agents', element: <Agents /> },
  { path: 'synthetics/agents/:agentId', element: <Agents /> },
  { path: 'synthetics/results', element: <Results /> },
  { path: 'synthetics/results/:resultId', element: <Results /> },
  { path: 'synthetics/assertions', element: <AssertionsLibrary /> },
  { path: 'synthetics/variables', element: <Variables /> },
  { path: 'synthetics/settings', element: <SyntheticsSettings /> },
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
