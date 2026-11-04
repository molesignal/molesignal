import { lazy, Suspense, type ComponentType } from 'react';
import {
  createBrowserRouter,
  Navigate,
  type RouteObject,
  useLocation,
  useParams,
  useSearchParams,
} from 'react-router-dom';

import { FirstOpenablePage, useOpenablePages } from '@/product/moduleAccess';
import { ReportRenderFrame } from '@/report-render/Frame';
import { isReportRender } from '@/report-render/mode';
import { QueryTabs } from '@/shell/queryTabs';

import { AccountSettingsLayout } from './account/AccountSettingsLayout';
import { AccountApiTokens } from './account/ApiTokens';
import { AccountNotify } from './account/Notify';
import { AccountPreferences } from './account/Preferences';
import { AccountProfile } from './account/Profile';
import { AccountSecurity } from './account/Security';
import { AccountSessions } from './account/Sessions';
import { AccountWorkspaceIdentity } from './account/WorkspaceIdentity';
import {
  AgentSettingsPage,
  ApprovalsPage as AgentApprovalsPage,
  AutomationsPage as AgentAutomationsPage,
  AgentQualityPage,
  ExecutionsPage as AgentExecutionsPage,
  AgentChat,
  AgentKnowledgePage,
  AgentLayout,
  DashboardDraftPage,
  ImLinkPage,
  InvestigationDetailPage,
  InvestigationsPage,
} from './agent';
import { Alerts } from './alerts';
import { AlertsAnomaly } from './alerts/Anomaly';
import { AlertsDiscovery } from './alerts/discovery';
import { AlertsEscalations } from './alerts/escalation';
import { AlertsHistory } from './alerts/History';
import { AlertsInhibitions } from './alerts/inhibition';
import { AlertsInsights } from './alerts/Insights';
import { ALERT_PAGES } from './alerts/Layout';
import { AlertsPostmortems } from './alerts/postmortems';
import { AlertsRemediation } from './alerts/remediation';
import { AlertsRouting } from './alerts/routing';
import { AlertRuleWorkbench } from './alerts/RuleWorkbench';
import { AlertsSchedules } from './alerts/schedule';
import { AlertsScheduleDetail } from './alerts/schedule/Detail';
import { AlertsSilences } from './alerts/silence';
import { AlertsSources } from './alerts/sources';
import { AlertsSourceDetail } from './alerts/sources/Detail';
import { AlertsTickets } from './alerts/tickets';
import { ApmCatalog, ApmErrors, ApmLayout, ApmOverview, ApmService, ApmServiceMap, ApmSettings, ApmSlos } from './apm';
import { apmIndexTarget, legacyServicesTarget } from './apm/compat';
import { APM_PAGES } from './apm/frame/Navigation';
import { Dashboards, DashboardView } from './dashboards';
import { DashboardEditor } from './dashboards/Editor';
import { DashboardImport } from './dashboards/Import';
import { DashboardNewPanel } from './dashboards/NewPanel';
import { Datasource } from './Datasource';
import { DefaultHomeRedirect } from './DefaultHomeRedirect';
import {
  ExtendTableDetail,
  ExtendTables,
  FunctionsList,
  FunctionsEdit,
} from './functions';
import { Home } from './home';
import {
  ApiTokens as IamApiTokens,
  Approvals as IamApprovals,
  EmailDomains as IamEmailDomains,
  Groups as IamGroups,
  IamIndexRedirect,
  IamLayout,
  Invitations as IamInvitations,
  Organizations as IamOrganizations,
  Policies as IamPolicies,
  Roles as IamRoles,
  ServiceAccounts as IamServiceAccounts,
  Teams as IamTeams,
  Users as IamUsers,
} from './iam';
import { IncidentDetail } from './IncidentDetail';
import { Investigate } from './Investigate';
import { Logs } from './logs';
import { LogInspections } from './logs/inspections';
import { LogInspectionDetail } from './logs/inspections/Detail';
import { LogsInspector } from './logs/Inspector';
import { Metrics } from './Metrics';
import { Noc } from './Noc';
import { NotFound } from './NotFound';
import { NotifyConnectorsPage } from './notify/connector';
import { NotifyDefaultsPage } from './notify/DefaultsPage';
import { NotifyDeliveriesPage } from './notify/DeliveriesPage';
import { NotifyPoliciesPage } from './notify/policy';
import { NotifyTemplatesPage } from './notify/template';
import { NotifyUsersPage } from './notify/UsersPage';
import { InboundMcpOAuthAuthorize } from './oauth/Authorize';
import {
  PipelineAdd,
  PipelineBackfill,
  PipelineConnectors,
  PipelineDetail,
  PipelineEdit,
  PipelineHistory,
  PipelineImport,
  Pipelines,
} from './pipelines';
import { ProfileDetail, Profiles, ProfilesCompare } from './profiles';
import { PublicShare } from './PublicShare';
import { Reports } from './reports';
import { RequireAuth } from './RequireAuth';
import { RouteError } from './RouteError';
import { RumLayout } from './rum';
import {
  LegacyApmUserExperienceRedirect,
  RUM_ROUTE_CHILDREN,
} from './rum/routes';
import { SavedViewsRoute } from './saved-views';
import { SemanticGroups } from './SemanticGroups';
import {
  AlertEvaluationSettings,
  HolidayCalendars,
  ImportanceSettings,
  Audit,
  AuditRetentionSettings,
  ClientIpSettings,
  SigninRequirementsSettings,
  CipherKeys,
  DomainManagement,
  FieldMasking,
  General,
  Nodes,
  OrganizationManagement,
  QueryManagement,
  RegexPatterns,
  SeriesLimitSettings,
  IntakeUsageSettings,
  SigninLimitSettings,
  SettingsIndexRedirect,
  SettingsLayout,
  SsoProviders,
  SsoSigninTestResult,
  StreamRetentionSettings,
} from './settings';
import { ShellRoot } from './ShellRoot';
import { Signin } from './Signin';
import { Signup } from './Signup';
import {
  PublicStatusHistoryPage,
  PublicStatusIncidentPage,
  PublicStatusPage,
  PublicStatusUptimePage,
} from './status-pages/public';
import { STATUS_PAGE_MANAGEMENT_ROUTES } from './status-pages/routes';
import { Streams } from './streams';
import { StreamExplore } from './streams/Explore';
import { SearchJobs } from './streams/jobs';
import { IntakeUsage } from './streams/usage';
import { SYNTHETICS_ROUTES } from './synthetics/routes';
import { Traces } from './traces';
import { TraceDetail } from './traces/Detail';
import { TraceSessionDetail } from './traces/SessionDetail';

// Demo routes for perf benchmarking. The `import.meta.env.DEV` guard lets
// Vite dead-code-eliminate every demo chunk from production builds — the
// dynamic `import()` lives inside a constant-false branch in prod so the
// entire `lazy()` call (including the imported module) is dropped.
type LazyComp = ComponentType<Record<string, unknown>>;
const TimeSeriesChartDemo: LazyComp | null = import.meta.env.DEV
  ? lazy(() => import('@/viz/_demo/TimeSeriesChart.demo').then((m) => ({ default: m.TimeSeriesChartDemo })))
  : null;
const TraceDemo: LazyComp | null = import.meta.env.DEV
  ? lazy(() => import('@/viz/_demo/Trace.demo').then((m) => ({ default: m.TraceDemo })))
  : null;
const TopologyDemo: LazyComp | null = import.meta.env.DEV
  ? lazy(() => import('@/viz/_demo/Topology.demo').then((m) => ({ default: m.TopologyDemo })))
  : null;
const LogStreamDemo: LazyComp | null = import.meta.env.DEV
  ? lazy(() => import('@/viz/_demo/LogStream.demo').then((m) => ({ default: m.LogStreamDemo })))
  : null;
const StatesDemo: LazyComp | null = import.meta.env.DEV
  ? lazy(() => import('@/viz/_demo/States.demo').then((m) => ({ default: m.StatesDemo })))
  : null;
const ProductTemplateDemo: LazyComp | null = import.meta.env.DEV
  ? lazy(() => import('@/product/templates.fixture').then((m) => ({ default: m.ProductTemplateFixture })))
  : null;

const demoRoute = (path: string, Comp: LazyComp | null): RouteObject[] =>
  Comp
    ? [{ path, element: <Suspense fallback={<div className="p-5 text-xs text-muted-foreground">Loading demo…</div>}><Comp /></Suspense> }]
    : [];

const DEMO_ROUTES: RouteObject[] = [
  ...demoRoute('/_demo/timeseries', TimeSeriesChartDemo),
  ...demoRoute('/_demo/trace', TraceDemo),
  ...demoRoute('/_demo/topology', TopologyDemo),
  ...demoRoute('/_demo/log', LogStreamDemo),
  ...demoRoute('/_demo/states', StatesDemo),
  ...demoRoute('/_demo/templates', ProductTemplateDemo),
];

function DatasourceRedirect() {
  const params = useParams();
  const category = params.category ? `/${params.category}` : '';
  const source = params.source ? `/${params.source}` : '';
  return <Navigate to={`/datasource${category}${source}`} replace />;
}

function ApmIndexRedirect() {
  const location = useLocation();
  const [landing] = useOpenablePages(APM_PAGES, (path) => path);
  return <Navigate to={apmIndexTarget(location.search, location.hash, landing)} replace />;
}

function LegacyServicesRedirect() {
  const location = useLocation();
  return (
    <Navigate
      to={legacyServicesTarget(location.pathname, location.search, location.hash)}
      replace
    />
  );
}

function AuthenticatedShell() {
  const [search] = useSearchParams();

  // A page the report renderer is about to print is that page's content alone.
  return (
    <RequireAuth>
      {isReportRender(search) ? <ReportRenderFrame /> : <ShellRoot />}
    </RequireAuth>
  );
}

function RootShellRoute() {
  const location = useLocation();
  if (location.pathname === '/') {
    return (
      <PublicStatusPage
        source="domain"
        unmatchedDomain={<AuthenticatedShell />}
      />
    );
  }
  return <AuthenticatedShell />;
}

/** The pages under the shell, behind one boundary: the shell's outlet draws the failure, its sidebar and top bar stay. */
function insideShell(children: RouteObject[]): RouteObject[] {
  return [{ errorElement: <RouteError />, children }];
}

/** What stands above the shell (sign-in, public status pages) takes the whole window when it fails. */
function aboveShell(routes: RouteObject[]): RouteObject[] {
  return [{ errorElement: <RouteError fullPage />, children: routes }];
}

export const router = createBrowserRouter(aboveShell([
  { path: '/signin', element: <Signin /> },
  { path: '/signup', element: <Signup /> },
  {
    path: '/oauth/authorize',
    element: (
      <RequireAuth>
        <InboundMcpOAuthAuthorize />
      </RequireAuth>
    ),
  },
  { path: '/shared', element: <PublicShare /> },
  {
    path: '/history',
    element: (
      <PublicStatusHistoryPage
        source="domain"
        unmatchedDomain={<Navigate to="/home" replace />}
      />
    ),
  },
  {
    path: '/uptime',
    element: (
      <PublicStatusUptimePage
        source="domain"
        unmatchedDomain={<Navigate to="/home" replace />}
      />
    ),
  },
  {
    path: '/incidents/:incidentId',
    element: (
      <PublicStatusIncidentPage
        source="domain"
        unmatchedDomain={<Navigate to="/home" replace />}
      />
    ),
  },
  {
    path: '/status/:slug/incidents/:incidentId',
    element: <PublicStatusIncidentPage />,
  },
  { path: '/status/:slug/history', element: <PublicStatusHistoryPage /> },
  { path: '/status/:slug/uptime', element: <PublicStatusUptimePage /> },
  { path: '/status/:slug', element: <PublicStatusPage /> },
  ...DEMO_ROUTES,

  {
    path: '/noc',
    element: (
      <RequireAuth>
        <Noc />
      </RequireAuth>
    ),
  },

  {
    path: '/',
    element: <RootShellRoute />,
    errorElement: <RouteError fullPage />,
    children: insideShell([
      { index: true, element: <DefaultHomeRedirect /> },

      /* OVERVIEW */
      { path: 'home', element: <Home /> },

      /* DATASOURCE — data-source onboarding guides */
      { path: 'datasource', element: <Datasource /> },
      { path: 'datasource/:category', element: <Datasource /> },
      { path: 'datasource/:category/:source', element: <Datasource /> },
      { path: 'datasources', element: <Navigate to="/datasource" replace /> },
      { path: 'intake', element: <Navigate to="/datasource" replace /> },
      { path: 'intake/:category', element: <DatasourceRedirect /> },
      { path: 'intake/:category/:source', element: <DatasourceRedirect /> },

      /* OBSERVE */
      {
        path: 'agent',
        element: <AgentLayout />,
        children: [
          { index: true, element: <Navigate to="/agent/chat" replace /> },
          { path: 'chat', element: <AgentChat /> },
          { path: 'investigations', element: <InvestigationsPage /> },
          { path: 'investigations/:id', element: <InvestigationDetailPage /> },
          { path: 'knowledge', element: <AgentKnowledgePage /> },
          { path: 'knowledge/:view', element: <AgentKnowledgePage /> },
          { path: 'automations', element: <AgentAutomationsPage /> },
          { path: 'quality', element: <AgentQualityPage /> },
          { path: 'quality/:view', element: <AgentQualityPage /> },
          { path: 'im/link', element: <ImLinkPage /> },
          { path: 'approvals', element: <AgentApprovalsPage /> },
          { path: 'executions', element: <AgentExecutionsPage /> },
          {
            path: 'settings',
            element: <AgentSettingsPage />,
          },
          {
            path: 'settings/:section',
            element: <AgentSettingsPage />,
          },
        ],
      },
      { path: 'ai/dashboard-drafts/:id', element: <DashboardDraftPage /> },
      // The explorers hold several queries at once, each in a tab of its own.
      { path: 'logs', element: <QueryTabs kind="logs"><Logs /></QueryTabs> },
      { path: 'logs/inspector', element: <LogsInspector /> },
      { path: 'logs/inspections', element: <LogInspections /> },
      { path: 'logs/inspections/:id', element: <LogInspectionDetail /> },
      { path: 'metrics', element: <QueryTabs kind="metrics"><Metrics /></QueryTabs> },
      { path: 'traces', element: <QueryTabs kind="traces"><Traces /></QueryTabs> },
      { path: 'traces/session/:id', element: <TraceSessionDetail /> },
      { path: 'traces/:id', element: <TraceDetail /> },
      { path: 'service-graph', element: <Navigate to="/traces?tab=service-graph" replace /> },
      {
        path: 'apm',
        element: <ApmLayout />,
        children: [
          { index: true, element: <ApmIndexRedirect /> },
          { path: 'overview', element: <ApmOverview /> },
          { path: 'services', element: <ApmCatalog /> },
          { path: 'map', element: <ApmServiceMap /> },
          { path: 'errors', element: <ApmErrors /> },
          { path: 'slos', element: <ApmSlos /> },
          { path: 'settings', element: <ApmSettings /> },
          { path: 'services/:service', element: <ApmService /> },
          { path: 'services/:service/:section', element: <ApmService /> },
          {
            path: 'user-experience/*',
            element: <LegacyApmUserExperienceRedirect />,
          },
        ],
      },
      { path: 'services', element: <LegacyServicesRedirect /> },
      { path: 'services/:service', element: <LegacyServicesRedirect /> },
      {
        path: 'rum',
        element: <RumLayout />,
        children: RUM_ROUTE_CHILDREN,
      },
      { path: 'dashboards', element: <Dashboards /> },
      { path: 'dashboards/new/edit', element: <DashboardEditor /> },
      { path: 'dashboards/import', element: <DashboardImport /> },
      { path: 'dashboards/:id', element: <DashboardView /> },
      { path: 'dashboards/:id/edit', element: <DashboardEditor /> },
      { path: 'dashboards/:id/panels/new', element: <DashboardNewPanel /> },
      { path: 'alerts', element: <FirstOpenablePage paths={ALERT_PAGES} /> },
      { path: 'alerts/incidents', element: <Alerts /> },
      { path: 'alerts/rules', element: <Alerts /> },
      { path: 'alerts/rules/new', element: <AlertRuleWorkbench /> },
      { path: 'alerts/rules/:id/edit', element: <AlertRuleWorkbench /> },
      { path: 'alerts/history', element: <AlertsHistory /> },
      { path: 'alerts/insights', element: <AlertsInsights /> },
      { path: 'alerts/anomaly/add', element: <AlertsAnomaly /> },
      { path: 'alerts/anomaly/edit/:id', element: <AlertsAnomaly /> },
      { path: 'alerts/escalations', element: <AlertsEscalations /> },
      { path: 'alerts/schedules', element: <AlertsSchedules /> },
      { path: 'alerts/schedules/:id', element: <AlertsScheduleDetail /> },
      { path: 'alerts/routing', element: <AlertsRouting /> },
      { path: 'alerts/silences', element: <AlertsSilences /> },
      { path: 'alerts/inhibitions', element: <AlertsInhibitions /> },
      { path: 'alerts/sources', element: <AlertsSources /> },
      { path: 'alerts/sources/:id', element: <AlertsSourceDetail /> },
      { path: 'alerts/remediation', element: <AlertsRemediation /> },
      { path: 'alerts/tickets', element: <AlertsTickets /> },
      { path: 'alerts/postmortems', element: <AlertsPostmortems /> },
      { path: 'alerts/discovery', element: <AlertsDiscovery /> },
      { path: 'alerts/semantic-groups', element: <SemanticGroups /> },
      // Alias for the planned /alerts/import-semantic-groups route.
      { path: 'alerts/import-semantic-groups', element: <SemanticGroups /> },
      ...SYNTHETICS_ROUTES,

      /* DATA */
      { path: 'streams', element: <Streams /> },
      // A fixed path outranks the stream detail route's `:id`, so "jobs" is never read as a stream.
      { path: 'streams/jobs', element: <SearchJobs /> },
      { path: 'streams/usage', element: <IntakeUsage /> },
      { path: 'streams/:id', element: <StreamExplore /> },
      { path: 'pipelines', element: <Pipelines /> },
      { path: 'pipelines/new', element: <PipelineAdd /> },
      { path: 'pipelines/import', element: <PipelineImport /> },
      { path: 'pipelines/connectors', element: <PipelineConnectors /> },
      { path: 'pipelines/:id', element: <PipelineDetail /> },
      { path: 'pipelines/:id/edit', element: <PipelineEdit /> },
      { path: 'pipelines/:id/history', element: <PipelineHistory /> },
      { path: 'pipelines/:id/backfill', element: <PipelineBackfill /> },
      { path: 'reports', element: <Reports /> },
      ...STATUS_PAGE_MANAGEMENT_ROUTES,
      { path: 'functions', element: <FunctionsList /> },
      { path: 'functions/new', element: <FunctionsEdit /> },
      { path: 'functions/:id', element: <FunctionsEdit /> },
      { path: 'extend-tables', element: <ExtendTables /> },
      { path: 'extend-tables/:table', element: <ExtendTableDetail /> },

      /* PROFILES */
      { path: 'profiles', element: <Profiles /> },
      { path: 'profiles/compare', element: <ProfilesCompare /> },
      { path: 'profiles/:id', element: <ProfileDetail /> },

      /* ADMIN */
      {
        path: 'iam',
        element: <IamLayout />,
        children: [
          { index: true, element: <IamIndexRedirect /> },
          { path: 'users', element: <IamUsers /> },
          { path: 'approvals', element: <IamApprovals /> },
          { path: 'api-tokens', element: <IamApiTokens /> },
          { path: 'service-accounts', element: <IamServiceAccounts /> },
          { path: 'organizations', element: <IamOrganizations /> },
          { path: 'groups', element: <IamGroups /> },
          {
            path: 'teams',
            element: <IamTeams />,
          },
          { path: 'roles', element: <IamRoles /> },
          { path: 'policies', element: <IamPolicies /> },
          { path: 'invitations', element: <IamInvitations /> },
          { path: 'email-domains', element: <IamEmailDomains /> },
          { path: 'sso', element: <SsoProviders /> },
          { path: 'sso/tests/:id', element: <SsoSigninTestResult /> },
        ],
      },
      {
        path: 'settings',
        element: <SettingsLayout />,
        children: [
          { index: true, element: <SettingsIndexRedirect /> },
          { path: 'general', element: <General /> },
          {
            path: 'organization',
            element: <Navigate to="/settings/general" replace />,
          },
          { path: 'client_ip', element: <ClientIpSettings /> },
          { path: 'signin_requirements', element: <SigninRequirementsSettings /> },
          { path: 'signin_limits', element: <SigninLimitSettings /> },
          { path: 'stream_retention', element: <StreamRetentionSettings /> },
          { path: 'series_limits', element: <SeriesLimitSettings /> },
          { path: 'intake_usage', element: <IntakeUsageSettings /> },
          { path: 'alert_evaluation', element: <AlertEvaluationSettings /> },
          {
            path: 'alerting',
            element: <Navigate to="/settings/alerting/holidays" replace />,
          },
          { path: 'alerting/holidays', element: <HolidayCalendars /> },
          { path: 'alerting/importance', element: <ImportanceSettings /> },
          { path: 'nodes', element: <Nodes /> },
          {
            path: 'notify',
            element: <Navigate to="/settings/notify/connectors" replace />,
          },
          { path: 'notify/connectors', element: <NotifyConnectorsPage /> },
          { path: 'notify/users', element: <NotifyUsersPage /> },
          { path: 'notify/policies', element: <NotifyPoliciesPage /> },
          { path: 'notify/templates', element: <NotifyTemplatesPage /> },
          { path: 'notify/defaults', element: <NotifyDefaultsPage /> },
          { path: 'notify/deliveries', element: <NotifyDeliveriesPage /> },
          { path: 'sso_providers', element: <Navigate to="/iam/sso" replace /> },
          { path: 'cipher_keys', element: <CipherKeys /> },
          { path: 'regex_patterns', element: <RegexPatterns /> },
          { path: 'field_masking', element: <FieldMasking /> },
          { path: 'domain_management', element: <DomainManagement /> },
          { path: 'organization_management', element: <OrganizationManagement /> },
          { path: 'query_management', element: <QueryManagement /> },
          {
            path: 'audit',
            element: <Audit />,
          },
          { path: 'audit_retention', element: <AuditRetentionSettings /> },
        ],
      },

      /* SAAS ACCOUNT */
      {
        path: 'account/settings',
        element: <AccountSettingsLayout />,
        children: [
          { index: true, element: <Navigate to="/account/settings/profile" replace /> },
          { path: 'profile', element: <AccountProfile /> },
          { path: 'preferences', element: <AccountPreferences /> },
          { path: 'notify', element: <AccountNotify /> },
          { path: 'security', element: <AccountSecurity /> },
          { path: 'sessions', element: <AccountSessions /> },
          { path: 'api-tokens', element: <AccountApiTokens /> },
          { path: 'workspace', element: <AccountWorkspaceIdentity /> },
        ],
      },

      /* Legacy and keyboard navigation routes. */
      { path: 'investigate', element: <Investigate /> },
      { path: 'alerts/incidents/:id', element: <IncidentDetail /> },
      { path: 'saved-views', element: <SavedViewsRoute /> },

      /* 认证区内未知子路由（含错拼的 settings/iam 子路径）：保留外壳渲染友好 404，
         而非 React Router 默认的整页 ErrorBoundary。 */
      { path: '*', element: <NotFound /> },
    ]),
  },

  /* 未知顶层路径回首页（RequireAuth 再决定是否跳登录）。 */
  { path: '*', element: <Navigate to="/home" replace /> },
]));
