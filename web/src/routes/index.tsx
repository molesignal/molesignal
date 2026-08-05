import { lazy, Suspense, type ComponentType } from 'react';
import {
  createBrowserRouter,
  Navigate,
  type RouteObject,
  useLocation,
  useParams,
} from 'react-router-dom';

import { AccountBilling, AccountSupport } from './account';
import { AccountSettingsLayout } from './account/AccountSettingsLayout';
import { AccountNotify } from './account/Notify';
import { AccountPreferences } from './account/Preferences';
import { AccountProfile } from './account/Profile';
import { AccountSecurity } from './account/Security';
import { AccountSessions } from './account/Sessions';
import { AccountWorkspaceIdentity } from './account/WorkspaceIdentity';
import { Alerts } from './alerts';
import { AlertsAnomaly } from './alerts/Anomaly';
import { AlertsEscalations } from './alerts/Escalations';
import { AlertsHistory } from './alerts/History';
import { AlertsInsights } from './alerts/Insights';
import { AlertRuleWorkbench } from './alerts/RuleWorkbench';
import { AlertsSchedules } from './alerts/schedule';
import { AlertsScheduleDetail } from './alerts/schedule/Detail';
import { AlertsSilences } from './alerts/Silences';
import {
  ApmDependencies,
  ApmDeployments,
  ApmErrorDetail,
  ApmErrors,
  ApmLayout,
  ApmOverview,
  ApmServiceDetail,
  ApmServiceRuntime,
  ApmServices,
  ApmTransactionDetail,
  ApmTransactions,
} from './apm';
import {
  apmIndexTarget,
  legacyServicesTarget,
  legacyVersionCompareTarget,
} from './apm/compat';
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
  EmailDomains as IamEmailDomains,
  Groups as IamGroups,
  IamIndexRedirect,
  IamLayout,
  Invitations as IamInvitations,
  Organizations as IamOrganizations,
  Quota as IamQuota,
  Roles as IamRoles,
  ServiceAccounts as IamServiceAccounts,
  Teams as IamTeams,
  Users as IamUsers,
  Approvals as IamApprovals,
} from './iam';
import { IncidentDetail } from './IncidentDetail';
import {
  AgentSettingsPage,
  ApprovalsPage as IntelligenceApprovalsPage,
  AutomationsPage as IntelligenceAutomationsPage,
  ExecutionsPage as IntelligenceExecutionsPage,
  IntelligenceChat,
  IntelligenceLayout,
  DashboardDraftPage,
  InvestigationDetailPage,
  InvestigationsPage,
} from './intelligence';
import { Investigate } from './Investigate';
import { Logs } from './logs';
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
import {
  PipelineAdd,
  PipelineBackfill,
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
import { RumLayout } from './rum';
import {
  LegacyApmUserExperienceRedirect,
  RUM_ROUTE_CHILDREN,
} from './rum/routes';
import { SavedViews } from './SavedViews';
import { SemanticGroups } from './SemanticGroups';
import {
  Audit,
  Billing,
  ClientIpSettings,
  CipherKeys,
  Correlation,
  DomainManagement,
  General,
  License,
  ModelPricing,
  Nodes,
  OrganizationManagement,
  PipelineDestinations,
  QueryManagement,
  RegexPatterns,
  SettingsLayout,
  SsoProviders,
} from './settings';
import { ShellRoot } from './ShellRoot';
import { Signin } from './Signin';
import { Signup } from './Signup';
import { Streams } from './streams';
import { StreamExplore } from './streams/Explore';
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
const TraceFlameDemo: LazyComp | null = import.meta.env.DEV
  ? lazy(() => import('@/viz/_demo/TraceFlame.demo').then((m) => ({ default: m.TraceFlameDemo })))
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
  ...demoRoute('/_demo/trace', TraceFlameDemo),
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
  return <Navigate to={apmIndexTarget(location.search, location.hash)} replace />;
}

function LegacyVersionCompareRedirect() {
  const location = useLocation();
  return (
    <Navigate
      to={legacyVersionCompareTarget(location.search, location.hash)}
      replace
    />
  );
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

export const router = createBrowserRouter([
  { path: '/signin', element: <Signin /> },
  { path: '/signup', element: <Signup /> },
  { path: '/shared', element: <PublicShare /> },
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
    element: (
      <RequireAuth>
        <ShellRoot />
      </RequireAuth>
    ),
    children: [
      { index: true, element: <DefaultHomeRedirect /> },

      /* OVERVIEW */
      { path: 'home', element: <Home /> },

      /* DATASOURCE — data-source onboarding guides */
      { path: 'datasource', element: <Datasource /> },
      { path: 'datasource/:category', element: <Datasource /> },
      { path: 'datasource/:category/:source', element: <Datasource /> },
      { path: 'datasources', element: <Navigate to="/datasource" replace /> },
      { path: 'ingest', element: <Navigate to="/datasource" replace /> },
      { path: 'ingest/:category', element: <DatasourceRedirect /> },
      { path: 'ingest/:category/:source', element: <DatasourceRedirect /> },

      /* OBSERVE */
      {
        path: 'intelligence',
        element: <IntelligenceLayout />,
        children: [
          { index: true, element: <Navigate to="/intelligence/chat" replace /> },
          { path: 'chat', element: <IntelligenceChat /> },
          { path: 'investigations', element: <InvestigationsPage /> },
          { path: 'investigations/:id', element: <InvestigationDetailPage /> },
          { path: 'automations', element: <IntelligenceAutomationsPage /> },
          { path: 'approvals', element: <IntelligenceApprovalsPage /> },
          { path: 'executions', element: <IntelligenceExecutionsPage /> },
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
      { path: 'logs', element: <Logs /> },
      { path: 'logs/inspector', element: <LogsInspector /> },
      { path: 'metrics', element: <Metrics /> },
      { path: 'traces', element: <Traces /> },
      { path: 'traces/session/:id', element: <TraceSessionDetail /> },
      { path: 'traces/:id', element: <TraceDetail /> },
      { path: 'service-graph', element: <Navigate to="/traces?tab=service-graph" replace /> },
      {
        path: 'apm',
        element: <ApmLayout />,
        children: [
          { index: true, element: <ApmIndexRedirect /> },
          { path: 'overview', element: <ApmOverview /> },
          { path: 'services', element: <ApmServices /> },
          { path: 'services/:service', element: <ApmServiceDetail /> },
          { path: 'services/:service/runtime', element: <ApmServiceRuntime /> },
          { path: 'transactions', element: <ApmTransactions /> },
          {
            path: 'transactions/:transaction',
            element: <ApmTransactionDetail />,
          },
          { path: 'dependencies', element: <ApmDependencies /> },
          { path: 'errors', element: <ApmErrors /> },
          { path: 'errors/:fingerprint', element: <ApmErrorDetail /> },
          { path: 'deployments', element: <ApmDeployments /> },
          {
            path: 'versions/compare',
            element: <LegacyVersionCompareRedirect />,
          },
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
      { path: 'alerts', element: <Navigate to="/alerts/incidents" replace /> },
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
      { path: 'alerts/silences', element: <AlertsSilences /> },
      { path: 'alerts/semantic-groups', element: <SemanticGroups /> },
      // sitemap parity alias for the planned /alerts/import-semantic-groups route.
      { path: 'alerts/import-semantic-groups', element: <SemanticGroups /> },

      /* DATA */
      { path: 'streams', element: <Streams /> },
      { path: 'streams/:id', element: <StreamExplore /> },
      { path: 'pipelines', element: <Pipelines /> },
      { path: 'pipelines/new', element: <PipelineAdd /> },
      { path: 'pipelines/import', element: <PipelineImport /> },
      { path: 'pipelines/connectors', element: <PipelineDestinations /> },
      { path: 'pipelines/:id', element: <PipelineDetail /> },
      { path: 'pipelines/:id/edit', element: <PipelineEdit /> },
      { path: 'pipelines/:id/history', element: <PipelineHistory /> },
      { path: 'pipelines/:id/backfill', element: <PipelineBackfill /> },
      { path: 'reports', element: <Reports /> },
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
          { path: 'service-accounts', element: <IamServiceAccounts /> },
          { path: 'organizations', element: <IamOrganizations /> },
          { path: 'groups', element: <IamGroups /> },
          {
            path: 'teams',
            element: <IamTeams />,
          },
          { path: 'roles', element: <IamRoles /> },
          { path: 'quota', element: <IamQuota /> },
          { path: 'invitations', element: <IamInvitations /> },
          { path: 'email-domains', element: <IamEmailDomains /> },
          { path: 'sso', element: <SsoProviders /> },
        ],
      },
      {
        path: 'settings',
        element: <SettingsLayout />,
        children: [
          { index: true, element: <Navigate to="/settings/general" replace /> },
          { path: 'general', element: <General /> },
          {
            path: 'organization',
            element: <Navigate to="/settings/general" replace />,
          },
          { path: 'license', element: <License /> },
          { path: 'billing', element: <Billing /> },
          { path: 'client_ip', element: <ClientIpSettings /> },
          { path: 'nodes', element: <Nodes /> },
          { path: 'correlation', element: <Correlation /> },
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
          { path: 'domain_management', element: <DomainManagement /> },
          { path: 'organization_management', element: <OrganizationManagement /> },
          { path: 'model_pricing', element: <ModelPricing /> },
          { path: 'query_management', element: <QueryManagement /> },
          {
            path: 'audit',
            element: <Audit />,
          },
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
          { path: 'workspace', element: <AccountWorkspaceIdentity /> },
        ],
      },
      { path: 'account/billing', element: <AccountBilling /> },
      { path: 'account/support', element: <AccountSupport /> },

      /* legacy / keyboard nav parity */
      { path: 'investigate', element: <Investigate /> },
      { path: 'alerts/incidents/:id', element: <IncidentDetail /> },
      { path: 'saved-views', element: <SavedViews /> },

      /* 认证区内未知子路由（含错拼的 settings/iam 子路径）：保留外壳渲染友好 404，
         而非 React Router 默认的整页 ErrorBoundary。 */
      { path: '*', element: <NotFound /> },
    ],
  },

  /* 未知顶层路径回首页（RequireAuth 再决定是否跳登录）。 */
  { path: '*', element: <Navigate to="/home" replace /> },
]);
