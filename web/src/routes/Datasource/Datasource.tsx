import { useQuery } from '@tanstack/react-query';
import {
  Activity,
  Bot,
  Braces,
  Check,
  CheckCircle2,
  Circle,
  Clock3,
  Cloud,
  Code2,
  Database,
  FileJson,
  Flame,
  Gauge,
  KeyRound,
  LoaderCircle,
  Monitor,
  MonitorCog,
  Network,
  RadioTower,
  RefreshCw,
  Router,
  Search,
  Send,
  Server,
  ServerCog,
  Shield,
  ShipWheel,
  Sparkles,
  Smartphone,
  Terminal,
  TriangleAlert,
  Webhook,
  Workflow,
  type LucideIcon,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';

import * as homeApi from '@/api/home';
import * as ingestionApi from '@/api/ingestion';
import * as rumIngestApi from '@/api/rumIngest';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton, Pill, type PillTone, uiLabelClass } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';
import { PageHeader } from '@/shell/PageHeader';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';

import { MarkdownCodeBlock } from './codeBlock/MarkdownCodeBlock';
import { TokenPanel } from './credentials/TokenPanel';
import {
  filterSources,
  PRIMARY_CATEGORIES,
  primaryCategoryFromRoute,
  summarizeSourceSignals,
  type IntegrationMethod,
  type PrimaryCategory,
  type SignalFilter,
  type SourceSignalSummary,
} from './datasourceModel';
import {
  type IngestContext,
  isValidRumApplicationId,
  substitute,
  useIngestContext,
} from './ingestContext';
import { ApplicationPanel } from './mobileRum/ApplicationPanel';
import {
  CATEGORIES,
  type Category,
  type GuideStep,
  type Signal,
  SOURCES,
  type Source,
} from './sources';
import {
  ingestPathForSignal,
  isIngestSignal,
} from '../streams/datasourceLink';

const SIGNAL_TONE: Record<Signal, PillTone> = {
  logs: 'orange',
  metrics: 'blue',
  traces: 'green',
  profiles: 'purple',
};

const SIGNALS: readonly Signal[] = ['logs', 'metrics', 'traces', 'profiles'];
const METHODS: readonly IntegrationMethod[] = ['all', 'native', 'otel', 'collector', 'api'];

const SIGNAL_FILTER_ON: Record<Signal, string> = {
  logs: 'border-orange/30 bg-orange-dim text-orange-soft',
  metrics: 'border-blue/30 bg-blue-dim text-blue-soft',
  traces: 'border-green/30 bg-green-dim text-green-soft',
  profiles: 'border-purple/30 bg-purple-dim text-purple-soft',
};

const SOURCE_ICONS: Record<string, LucideIcon> = {
  kubernetes: ShipWheel,
  linux: Server,
  windows: Monitor,
  aws: Cloud,
  gcp: Cloud,
  azure: Cloud,
  'continuous-profiling': Flame,
  rum: MonitorCog,
  'rum-flutter': Smartphone,
  'rum-android': Smartphone,
  'rum-ios': Smartphone,
  curl: Terminal,
  'bulk-ndjson': FileJson,
  opentelemetry: Activity,
  'otel-collector': Router,
  nginx: ServerCog,
  apache: ServerCog,
  haproxy: Router,
  postgres: Database,
  mysql: Database,
  mongodb: Database,
  clickhouse: Database,
  redis: Database,
  falco: Shield,
  osquery: Terminal,
  crowdstrike: Shield,
  'github-actions': Workflow,
  argocd: Workflow,
  jenkins: Workflow,
  envoy: Network,
  traefik: Router,
  cloudflare: Cloud,
  kafka: Send,
  rabbitmq: Send,
  nats: RadioTower,
  python: Code2,
  go: Code2,
  java: Code2,
  node: Code2,
  rust: Code2,
  dotnet: Code2,
  openai: Bot,
  anthropic: Sparkles,
  langchain: Workflow,
  llamaindex: Database,
  webhook: Webhook,
  'graphql-subscription': Braces,
};

const CATEGORY_ICONS: Record<Category, LucideIcon> = {
  recommended: Activity,
  otel: Activity,
  'otel-collector': Router,
  custom: Terminal,
  servers: Server,
  databases: Database,
  security: Shield,
  devops: Workflow,
  networking: Network,
  queues: Send,
  languages: Code2,
  ai: Bot,
};

const DEFAULT_STREAM = 'default';
const OVERVIEW_WINDOW_SECS = 60 * 60;

interface HealthProbe {
  ok: boolean;
  latencyMs: number;
  message?: string;
}

type ValidationState =
  | { kind: 'idle' }
  | { kind: 'pending' }
  | {
      kind: 'success' | 'warning';
      health: HealthProbe;
      summary: SourceSignalSummary;
    }
  | { kind: 'error'; message: string };

export function Datasource() {
  const { t } = useTranslation('onboarding');
  const params = useParams();
  const navigate = useNavigate();
  const apiTokensReadAccess = useActionAccess({
    permission: 'api_tokens.read',
  });
  const streamCreateAccess = useActionAccess({
    permission: 'streams.create',
  });
  const [searchParams] = useSearchParams();
  const activeCategory = primaryCategoryFromRoute(params.category);
  const [search, setSearch] = React.useState('');
  const [method, setMethod] = React.useState<IntegrationMethod>('all');
  const signalParam = searchParams.get('signal');
  const requestedSignal = isIngestSignal(signalParam) ? signalParam : null;
  const requestedStream = searchParams.get('stream')?.trim() || DEFAULT_STREAM;
  const [signal, setSignal] = React.useState<SignalFilter>(requestedSignal ?? 'all');
  const [verifiedSources, setVerifiedSources] = React.useState<Set<string>>(new Set());

  React.useEffect(() => {
    setSignal(requestedSignal ?? 'all');
  }, [requestedSignal]);

  const overviewQuery = useQuery({
    queryKey: ['home-overview', 'datasource', OVERVIEW_WINDOW_SECS],
    queryFn: () =>
      homeApi.overview({
        windowSecs: OVERVIEW_WINDOW_SECS,
        bucketCount: 6,
      }),
    staleTime: 15_000,
    refetchInterval: 30_000,
  });

  const visible = React.useMemo(
    () =>
      filterSources({
        sources: SOURCES,
        category: activeCategory,
        method,
        signal,
        query: search,
      }),
    [activeCategory, method, search, signal],
  );

  const routedSource = params.source
    ? SOURCES.find((source) => source.id === params.source)
    : undefined;
  const selected =
    routedSource && visible.some((source) => source.id === routedSource.id)
      ? routedSource
      : visible[0];

  React.useEffect(() => {
    if ((!params.source || !selected) && visible[0]) {
      navigate(`/datasource/${activeCategory}/${visible[0].id}`, { replace: true });
    }
  }, [activeCategory, navigate, params.source, selected, visible]);

  const switchCategory = (category: PrimaryCategory) => {
    setSearch('');
    setMethod('all');
    setSignal('all');
    const first = filterSources({
      sources: SOURCES,
      category,
      method: 'all',
      signal: 'all',
      query: '',
    })[0];
    navigate(first ? `/datasource/${category}/${first.id}` : `/datasource/${category}`);
  };

  const switchMethod = (next: IntegrationMethod) => {
    setMethod(next);
    const first = filterSources({
      sources: SOURCES,
      category: activeCategory,
      method: next,
      signal,
      query: search,
    })[0];
    if (first) navigate(`/datasource/${activeCategory}/${first.id}`);
  };

  const switchSignal = (next: SignalFilter) => {
    setSignal(next);
    const first = filterSources({
      sources: SOURCES,
      category: activeCategory,
      method,
      signal: next,
      query: search,
    })[0];
    if (first) navigate(`/datasource/${activeCategory}/${first.id}`);
  };

  const refreshOverview = React.useCallback(async (): Promise<homeApi.HomeOverview> => {
    const result = await overviewQuery.refetch();
    if (result.error) throw result.error;
    if (!result.data) throw new Error(t('datasource_page.validation_overview_unavailable'));
    return result.data;
  }, [overviewQuery, t]);

  return (
    <section className="flex h-[calc(100vh-var(--topbar-h)-var(--contextbar-h,0px))] min-h-0 flex-col overflow-hidden bg-bg-0">
      <PageHeader
        title={t('datasource_page.title')}
        subtitle={t('datasource_page.subtitle')}
        className="shrink-0 py-4"
        toolbar={
          <>
            <ChromeButton
              disabled={apiTokensReadAccess.disabled}
              disabledReason={apiTokensReadAccess.reason}
              onClick={() => navigate('/iam/service-accounts')}
            >
              <KeyRound className="h-3.5 w-3.5" />
              {t('datasource_page.api_tokens')}
            </ChromeButton>
            <ChromeButton
              variant="primary"
              disabled={streamCreateAccess.disabled}
              disabledReason={streamCreateAccess.reason}
              onClick={() => navigate('/datasource/custom/webhook')}
            >
              {t('datasource_page.custom_source')}
            </ChromeButton>
          </>
        }
      />

      <div className="shrink-0 border-b border-bd-0 bg-bg-1 px-4 py-3">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          <label className="flex h-9 min-w-[260px] flex-1 items-center gap-2 rounded-md border border-bd-1 bg-bg-2 px-3 lg:max-w-[560px]">
            <Search className="h-3.5 w-3.5 shrink-0 text-tx-3" />
            <input
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t('datasource_page.search_placeholder')}
              className="min-w-0 flex-1 bg-transparent font-sans text-sm text-tx-0 placeholder:text-tx-3 focus:outline-none"
            />
          </label>
          <Select value={method} onValueChange={(value) => switchMethod(value as IntegrationMethod)}>
            <SelectTrigger
              className="h-9 w-[168px] bg-bg-2 font-sans text-xs"
              aria-label={t('datasource_page.method_filter')}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {METHODS.map((item) => (
                <SelectItem key={item} value={item} className="text-xs">
                  {t(`datasource_page.methods.${item}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <span className="ml-auto hidden items-center gap-1.5 font-sans text-xs text-tx-3 xl:flex">
            <RefreshCw
              className={cn('h-3 w-3', overviewQuery.isFetching && 'animate-spin')}
            />
            {t('datasource_page.status_auto_refresh')}
          </span>
        </div>
        <nav
          className="mt-2 flex min-w-0 items-center gap-0.5 overflow-x-auto"
          aria-label={t('datasource_page.category_navigation')}
        >
          {PRIMARY_CATEGORIES.map((category) => (
            <button
              key={category}
              type="button"
              onClick={() => switchCategory(category)}
              aria-current={category === activeCategory ? 'page' : undefined}
              className={cn(
                'h-8 shrink-0 rounded-md px-3 font-sans text-xs font-strong transition-colors',
                category === activeCategory
                  ? 'bg-indigo-dim text-indigo-soft'
                  : 'text-tx-2 hover:bg-bg-2 hover:text-tx-0',
              )}
            >
              {t(`datasource_page.categories.${category}`)}
            </button>
          ))}
        </nav>
      </div>

      <div className="flex min-h-0 flex-1 overflow-hidden">
        <aside className="flex w-[280px] shrink-0 flex-col overflow-hidden border-r border-bd-0 bg-bg-1">
          <div className="border-b border-bd-0 px-3 py-2.5">
            <div className="flex items-center justify-between gap-2">
              <span className={uiLabelClass}>
                {search
                  ? t('datasource_page.matching', { count: visible.length })
                  : t(`datasource_page.categories.${activeCategory}`)}
              </span>
              <span className="font-mono text-xs text-tx-3">{visible.length}</span>
            </div>
            <div className="mt-2 flex flex-wrap items-center gap-1">
              <SignalFilterButton
                active={signal === 'all'}
                onClick={() => switchSignal('all')}
              >
                {t('datasource_page.signals.all')}
              </SignalFilterButton>
              {SIGNALS.map((item) => (
                <SignalFilterButton
                  key={item}
                  active={signal === item}
                  tone={item}
                  onClick={() => switchSignal(item)}
                >
                  {t(`datasource_page.signals.${item}`)}
                </SignalFilterButton>
              ))}
            </div>
          </div>

          <div className="min-h-0 flex-1 overflow-auto">
            {visible.length === 0 && (
              <div className="px-4 py-8 text-center font-sans text-xs leading-relaxed text-tx-2">
                {search
                  ? t('datasource_page.no_match', { search })
                  : t('datasource_page.no_match_filter')}
              </div>
            )}
            {visible.map((source) => (
              <SourceListItem
                key={source.id}
                source={source}
                selected={selected?.id === source.id}
                verified={verifiedSources.has(source.id)}
                summary={summarizeSourceSignals(source, overviewQuery.data)}
                onClick={() =>
                  navigate(`/datasource/${activeCategory}/${source.id}`)
                }
              />
            ))}
          </div>
        </aside>

        <main className="min-h-0 min-w-0 flex-1 overflow-auto">
          {selected ? (
            <Guide
              key={`${selected.id}:${signal}:${requestedStream}`}
              source={selected}
              signal={signal === 'all' ? undefined : signal}
              streamName={requestedStream}
              overview={overviewQuery.data}
              refreshOverview={refreshOverview}
              onVerified={() =>
                setVerifiedSources((current) => {
                  const next = new Set(current);
                  next.add(selected.id);
                  return next;
                })
              }
            />
          ) : (
            <EmptyState />
          )}
        </main>
      </div>
    </section>
  );
}

function SignalFilterButton({
  active,
  tone,
  children,
  onClick,
}: {
  active: boolean;
  tone?: Signal;
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      className={cn(
        'h-6 rounded border px-2 font-sans text-xs font-semibold transition-colors',
        active && tone && SIGNAL_FILTER_ON[tone],
        active && !tone && 'border-indigo/30 bg-indigo-dim text-indigo-soft',
        !active && 'border-bd-0 bg-bg-2 text-tx-3 hover:border-bd-1 hover:text-tx-1',
      )}
    >
      {children}
    </button>
  );
}

function SourceListItem({
  source,
  selected,
  verified,
  summary,
  onClick,
}: {
  source: Source;
  selected: boolean;
  verified: boolean;
  summary: SourceSignalSummary;
  onClick: () => void;
}) {
  const { t } = useTranslation('onboarding');
  const status = sourceStatus(verified, summary);
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        'group flex w-full items-center gap-2.5 border-b border-bd-0 border-l-2 px-3 py-2.5 text-left',
        selected
          ? 'border-l-indigo bg-indigo-dim/50'
          : 'border-l-transparent hover:bg-bg-2',
      )}
    >
      <SourceIcon source={source} selected={selected} size="list" />
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-2">
          <span className="min-w-0 flex-1 truncate font-sans text-xs font-strong text-tx-0">
            {source.name}
          </span>
          <span
            className={cn('h-1.5 w-1.5 shrink-0 rounded-full', status.dotClass)}
            title={t(status.labelKey)}
          />
        </span>
        <span className="mt-0.5 flex min-w-0 items-center gap-1.5 font-sans text-xs text-tx-3">
          <span className="truncate">
            {source.signals
              .map((signal) => t(`datasource_page.signals.${signal}`))
              .join(' · ')}
          </span>
          <span aria-hidden>·</span>
          <span className={cn('shrink-0', status.textClass)}>
            {t(status.labelKey)}
          </span>
        </span>
      </span>
    </button>
  );
}

function sourceStatus(verified: boolean, summary: SourceSignalSummary) {
  if (verified) {
    return {
      labelKey: 'datasource_page.source_status.verified',
      dotClass: 'bg-green',
      textClass: 'text-green-soft',
    };
  }
  if (summary.rows > 0 && summary.status === 'healthy') {
    return {
      labelKey: 'datasource_page.source_status.signal_active',
      dotClass: 'bg-green',
      textClass: 'text-green-soft',
    };
  }
  if (summary.rows > 0 && summary.status === 'delayed') {
    return {
      labelKey: 'datasource_page.source_status.delayed',
      dotClass: 'bg-yellow',
      textClass: 'text-yellow-soft',
    };
  }
  if (summary.status === 'degraded') {
    return {
      labelKey: 'datasource_page.source_status.attention',
      dotClass: 'bg-red',
      textClass: 'text-red-soft',
    };
  }
  return {
    labelKey: 'datasource_page.source_status.unverified',
    dotClass: 'bg-tx-3',
    textClass: 'text-tx-3',
  };
}

function SourceIcon({
  source,
  selected = false,
  size,
}: {
  source: Source;
  selected?: boolean;
  size: 'list' | 'hero';
}) {
  const Icon = SOURCE_ICONS[source.id] ?? CATEGORY_ICONS[source.category] ?? Activity;
  const hero = size === 'hero';
  return (
    <span
      className={cn(
        'grid shrink-0 place-items-center border transition-colors',
        hero ? 'h-12 w-12 rounded-lg' : 'h-8 w-8 rounded-md',
        selected
          ? 'border-indigo/25 bg-indigo-dim text-indigo-soft'
          : 'border-bd-0 bg-bg-2 text-tx-2 group-hover:border-bd-1 group-hover:text-tx-0',
      )}
      aria-hidden="true"
      title={source.name}
    >
      <Icon className={hero ? 'h-5 w-5 stroke-[1.8]' : 'h-4 w-4 stroke-[1.8]'} />
    </span>
  );
}

function Guide({
  source,
  signal,
  streamName,
  overview,
  refreshOverview,
  onVerified,
}: {
  source: Source;
  signal: Signal | undefined;
  streamName: string;
  overview: homeApi.HomeOverview | undefined;
  refreshOverview: () => Promise<homeApi.HomeOverview>;
  onVerified: () => void;
}) {
  const { t } = useTranslation('onboarding');
  const navigate = useNavigate();
  const [guideParams] = useSearchParams();
  const pipelineCreateAccess = useActionAccess({
    permission: 'pipelines.create',
  });
  const isRum = source.rumPlatform != null;
  const initialApplicationId = isRum ? (guideParams.get('app')?.trim() ?? '') : '';
  const [applicationId, setApplicationId] = React.useState(initialApplicationId);
  const [configuredApplicationId, setConfiguredApplicationId] = React.useState(
    initialApplicationId,
  );
  const context = useIngestContext({
    isRum,
    applicationId: configuredApplicationId,
  });
  const validationRef = React.useRef<HTMLElement>(null);
  const [deploymentConfirmed, setDeploymentConfirmed] = React.useState(false);
  const [validation, setValidation] = React.useState<ValidationState>({ kind: 'idle' });
  const [completed, setCompleted] = React.useState(false);
  const endpoint = context.endpoint + endpointForSource(source, signal, streamName);
  const prepared =
    context.applicationValid &&
    !context.tokenLoading &&
    !!context.token &&
    !context.tokenError;
  const verified = validation.kind === 'success';
  const stepsComplete = [prepared, deploymentConfirmed, verified, completed];
  const completedCount = stepsComplete.filter(Boolean).length;
  const currentStep = Math.min(
    stepsComplete.findIndex((item) => !item) + 1 || stepsComplete.length,
    4,
  );
  const deploymentSteps = source.steps.filter((step) => !isVerificationStep(step));
  const verificationSteps = source.steps.filter(isVerificationStep);

  const runValidation = async () => {
    setValidation({ kind: 'pending' });
    try {
      const [health, summary] = await Promise.all([
        probeHealth(),
        isRum
          ? rumIngestApi
              .recentErrorSummary({
                orgId: context.orgId,
                applicationId: context.applicationId,
              })
              .then(rumSourceSummary)
          : refreshOverview().then((nextOverview) =>
              summarizeSourceSignals(source, nextOverview),
            ),
      ]);
      if (!health.ok) {
        setValidation({
          kind: 'error',
          message: health.message ?? t('datasource_page.validation_endpoint_failed'),
        });
        return;
      }
      if (summary.rows > 0 && summary.lastReceivedAtMicros != null) {
        setDeploymentConfirmed(true);
        setValidation({
          kind: summary.status === 'healthy' ? 'success' : 'warning',
          health,
          summary,
        });
        if (summary.status === 'healthy') onVerified();
      } else {
        setValidation({ kind: 'warning', health, summary });
      }
    } catch (error) {
      setValidation({ kind: 'error', message: toApiError(error).message });
    }
  };

  const finish = () => {
    if (!verified) return;
    setCompleted(true);
  };

  return (
    <div className="mx-auto w-full max-w-[1040px] px-6 py-5">
      <header className="flex flex-col gap-4 border-b border-bd-0 pb-5 xl:flex-row xl:items-start">
        <div className="flex min-w-0 flex-1 items-start gap-3">
          <SourceIcon source={source} selected size="hero" />
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h1 className="m-0 font-sans text-xl font-display-strong tracking-tight text-tx-0">
                {t('datasource_page.connect_title', { source: source.name })}
              </h1>
              {source.signals.map((signal) => (
                <Pill key={signal} tone={SIGNAL_TONE[signal]}>
                  {t(`datasource_page.signals.${signal}`)}
                </Pill>
              ))}
            </div>
            <p className="mt-1.5 max-w-[700px] text-sm leading-relaxed text-tx-2">
              {source.description}
            </p>
          </div>
        </div>
        <div className="flex shrink-0 flex-wrap items-center gap-2 xl:justify-end">
          <ChromeButton
            onClick={() => {
              validationRef.current?.scrollIntoView({ behavior: 'smooth', block: 'start' });
            }}
          >
            <Gauge className="h-3.5 w-3.5" />
            {t('datasource_page.view_receive_status')}
          </ChromeButton>
          <ChromeButton
            variant="primary"
            disabled={validation.kind === 'pending' || !prepared}
            onClick={runValidation}
          >
            {validation.kind === 'pending' ? (
              <LoaderCircle className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <RefreshCw className="h-3.5 w-3.5" />
            )}
            {t('datasource_page.verify_connection')}
          </ChromeButton>
        </div>
      </header>

      <div className="border-b border-bd-0 py-4">
        <div className="mb-2 flex items-center justify-between gap-3">
          <span className={uiLabelClass}>{t('datasource_page.setup_progress')}</span>
          <span className="font-mono text-xs font-semibold text-indigo-soft">
            {t('datasource_page.progress_count', {
              completed: completedCount,
              total: 4,
              current: currentStep,
            })}
          </span>
        </div>
        <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
          {([
            ['prepare', prepared],
            ['deploy', deploymentConfirmed],
            ['verify', verified],
            ['complete', completed],
          ] as const).map(([key, done], index) => (
            <div
              key={key}
              className={cn(
                'flex min-h-11 items-center gap-2.5 rounded-md border px-3',
                done
                  ? 'border-green/25 bg-green-dim text-green-soft'
                  : index + 1 === currentStep
                    ? 'border-indigo/30 bg-indigo-dim text-indigo-soft'
                    : 'border-bd-0 bg-bg-1 text-tx-3',
              )}
            >
              {done ? (
                <CheckCircle2 className="h-4 w-4 shrink-0" />
              ) : (
                <Circle className="h-4 w-4 shrink-0" />
              )}
              <span className="min-w-0">
                <span className="block font-mono text-xs opacity-70">0{index + 1}</span>
                <span className="block truncate font-sans text-xs font-strong">
                  {t(`datasource_page.steps.${key}.title`)}
                </span>
              </span>
            </div>
          ))}
        </div>
      </div>

      <WizardSection
        number={1}
        title={t('datasource_page.steps.prepare.title')}
        description={t('datasource_page.steps.prepare.description')}
        status={prepared ? 'complete' : 'active'}
      >
        <div
          className={cn(
            'grid min-w-0 gap-3',
            isRum ? 'xl:grid-cols-3' : 'xl:grid-cols-2',
          )}
        >
          <EndpointPanel endpoint={endpoint} />
          {isRum && (
            <ApplicationPanel
              value={applicationId}
              valid={isValidRumApplicationId(applicationId)}
              confirmed={
                isValidRumApplicationId(applicationId) &&
                configuredApplicationId !== '' &&
                configuredApplicationId === applicationId.trim()
              }
              onChange={(value) => {
                setApplicationId(value);
                setConfiguredApplicationId('');
                setDeploymentConfirmed(false);
                setValidation({ kind: 'idle' });
                setCompleted(false);
              }}
              onConfirm={() => setConfiguredApplicationId(applicationId.trim())}
            />
          )}
          <TokenPanel context={context} />
        </div>
      </WizardSection>

      <WizardSection
        number={2}
        title={t('datasource_page.steps.deploy.title')}
        description={t('datasource_page.steps.deploy.description')}
        status={deploymentConfirmed ? 'complete' : prepared ? 'active' : 'pending'}
      >
        <div className="flex min-w-0 flex-col gap-4">
          {deploymentSteps.map((step, index) => (
            <Step key={`${step.title}-${index}`} index={index + 1} step={step} context={context} />
          ))}
          <label className="flex cursor-pointer items-start gap-2.5 rounded-md border border-bd-0 bg-bg-1 px-3 py-2.5">
            <input
              type="checkbox"
              checked={deploymentConfirmed}
              onChange={(event) => setDeploymentConfirmed(event.target.checked)}
              className="mt-0.5 h-4 w-4 accent-indigo"
            />
            <span>
              <span className="block font-sans text-xs font-strong text-tx-0">
                {t('datasource_page.deployment_confirm')}
              </span>
              <span className="mt-0.5 block font-sans text-xs leading-relaxed text-tx-2">
                {t('datasource_page.deployment_confirm_hint')}
              </span>
            </span>
          </label>
        </div>
      </WizardSection>

      <WizardSection
        ref={validationRef}
        number={3}
        title={t('datasource_page.steps.verify.title')}
        description={t('datasource_page.steps.verify.description')}
        status={verified ? 'complete' : deploymentConfirmed ? 'active' : 'pending'}
      >
        <ValidationPanel
          source={source}
          validation={validation}
          passiveSummary={
            isRum ? rumSourceSummary() : summarizeSourceSignals(source, overview)
          }
          verificationSteps={verificationSteps}
          context={context}
          onValidate={runValidation}
        />
      </WizardSection>

      <WizardSection
        number={4}
        title={t('datasource_page.steps.complete.title')}
        description={t('datasource_page.steps.complete.description')}
        status={completed ? 'complete' : verified ? 'active' : 'pending'}
      >
        {completed ? (
          <div className="flex flex-col gap-3 rounded-lg border border-green/30 bg-green-dim px-4 py-4 sm:flex-row sm:items-center">
            <span className="grid h-9 w-9 shrink-0 place-items-center rounded-full bg-green/15 text-green-soft">
              <Check className="h-4 w-4 stroke-[3]" />
            </span>
            <div className="min-w-0 flex-1">
              <div className="font-sans text-sm font-bold text-green-soft">
                {t('datasource_page.connection_complete', { source: source.name })}
              </div>
              <div className="mt-0.5 font-sans text-xs leading-relaxed text-tx-1">
                {t('datasource_page.connection_complete_hint')}
              </div>
            </div>
            <div className="flex flex-wrap gap-2">
              <ChromeButton onClick={() => navigate(viewPathForSource(source))}>
                {t('datasource_page.view_received_data')}
              </ChromeButton>
              <ChromeButton
                variant="primary"
                disabled={pipelineCreateAccess.disabled}
                disabledReason={pipelineCreateAccess.reason}
                onClick={() => navigate('/pipelines/new')}
              >
                {t('datasource_page.create_pipeline')}
              </ChromeButton>
            </div>
          </div>
        ) : (
          <div className="flex flex-col gap-3 rounded-md border border-bd-0 bg-bg-1 px-4 py-3 sm:flex-row sm:items-center">
            <div className="min-w-0 flex-1">
              <div className="font-sans text-xs font-strong text-tx-0">
                {verified
                  ? t('datasource_page.ready_to_complete')
                  : t('datasource_page.complete_requires_validation')}
              </div>
              <div className="mt-0.5 font-sans text-xs leading-relaxed text-tx-2">
                {t('datasource_page.complete_hint')}
              </div>
            </div>
            <ChromeButton
              variant="primary"
              disabled={!verified}
              onClick={finish}
              className="disabled:cursor-not-allowed disabled:opacity-45"
            >
              {t('datasource_page.complete_setup')}
            </ChromeButton>
          </div>
        )}
      </WizardSection>

      <footer className="flex flex-wrap items-center justify-between gap-2 border-t border-bd-0 py-4 font-sans text-xs text-tx-3">
        <span>
          {t('datasource_page.catalogue_meta', {
            category: categoryLabel(source.category),
          })}
        </span>
        {source.docsUrl && (
          <a
            href={source.docsUrl}
            target="_blank"
            rel="noreferrer"
            className="text-indigo-soft hover:underline"
          >
            {t('datasource_page.docs')}
          </a>
        )}
      </footer>
    </div>
  );
}

const WizardSection = React.forwardRef<
  HTMLElement,
  {
    number: number;
    title: string;
    description: string;
    status: 'complete' | 'active' | 'pending';
    children: React.ReactNode;
  }
>(function WizardSection({ number, title, description, status, children }, ref) {
  const { t } = useTranslation('onboarding');
  return (
    <section ref={ref} className="scroll-mt-4 border-b border-bd-0 py-6">
      <div className="mb-4 flex items-start gap-3">
        <span
          className={cn(
            'grid h-7 w-7 shrink-0 place-items-center rounded-full border font-mono text-xs font-bold',
            status === 'complete' && 'border-green/30 bg-green-dim text-green-soft',
            status === 'active' && 'border-indigo/30 bg-indigo-dim text-indigo-soft',
            status === 'pending' && 'border-bd-1 bg-bg-1 text-tx-3',
          )}
        >
          {status === 'complete' ? <Check className="h-3.5 w-3.5 stroke-[3]" /> : number}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h2 className="m-0 font-sans text-sm font-bold text-tx-0">{title}</h2>
            <span
              className={cn(
                'font-sans text-xs font-strong',
                status === 'complete' && 'text-green-soft',
                status === 'active' && 'text-indigo-soft',
                status === 'pending' && 'text-tx-3',
              )}
            >
              {t(`datasource_page.step_status.${status}`)}
            </span>
          </div>
          <p className="mt-1 max-w-[760px] font-sans text-xs leading-relaxed text-tx-2">
            {description}
          </p>
        </div>
      </div>
      <div className="min-w-0 pl-10">{children}</div>
    </section>
  );
});

function endpointForSource(
  source: Source,
  signal: Signal | undefined,
  streamName: string,
): string {
  if (source.rumPlatform) {
    return '/api/v1/rum/errors';
  }
  if (signal && source.signals.includes(signal)) {
    return ingestPathForSignal(signal, streamName);
  }
  if (source.signals.includes('profiles')) {
    return ingestPathForSignal('profiles', streamName);
  }
  if (source.signals.includes('traces')) {
    return ingestPathForSignal('traces', streamName);
  }
  if (source.signals.includes('metrics')) {
    return ingestPathForSignal('metrics', streamName);
  }
  return ingestPathForSignal('logs', streamName);
}

function categoryLabel(category: Category): string {
  return CATEGORIES.find((item) => item.id === category)?.label ?? category;
}

function EndpointPanel({ endpoint }: { endpoint: string }) {
  const { t } = useTranslation('onboarding');
  const [copied, setCopied] = React.useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(endpoint);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access can be denied by the browser.
    }
  };
  return (
    <div className="min-w-0 rounded-md border border-bd-0 bg-bg-1 p-3">
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className={uiLabelClass}>{t('datasource.endpoint')}</span>
        <span className="font-sans text-xs text-green-soft">
          {t('datasource_page.ready')}
        </span>
      </div>
      <div className="flex min-w-0 items-center gap-2">
        <code className="min-w-0 flex-1 truncate rounded border border-bd-0 bg-bg-2 px-2.5 py-2 font-mono text-xs text-tx-1">
          {endpoint}
        </code>
        <CopyIconButton
          onClick={copy}
          label={t('datasource.copy_endpoint')}
          copied={copied}
          copiedLabel={t('datasource_page.copied')}
        />
      </div>
      <p className="mt-2 font-sans text-xs leading-relaxed text-tx-2">
        {t('datasource.endpoint_description')}
      </p>
    </div>
  );
}

function Step({
  index,
  step,
  context,
}: {
  index: number;
  step: GuideStep;
  context: IngestContext;
}) {
  const title = step.title.replace(/^\s*\d+[.)、]\s*/, '');
  return (
    <div className="min-w-0">
      <div className="mb-2 flex items-center gap-2">
        <span className="font-mono text-xs font-semibold text-tx-3">
          {String(index).padStart(2, '0')}
        </span>
        <div className="font-sans text-xs font-strong text-tx-0">{title}</div>
      </div>
      {step.description && (
        <div className="mb-2 max-w-[760px] pl-6 font-sans text-xs leading-relaxed text-tx-2">
          {step.description}
        </div>
      )}
      {step.code && (
        <div className="pl-6">
          <MarkdownCodeBlock
            language={step.code.lang}
            content={substitute(step.code.content, context)}
          />
        </div>
      )}
      {step.note && (
        <div className="ml-6 mt-2 flex items-start gap-2 rounded-md border border-yellow/30 bg-yellow-dim p-2.5 font-sans text-xs text-yellow-soft">
          <TriangleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span>{step.note}</span>
        </div>
      )}
    </div>
  );
}

function ValidationPanel({
  source,
  validation,
  passiveSummary,
  verificationSteps,
  context,
  onValidate,
}: {
  source: Source;
  validation: ValidationState;
  passiveSummary: SourceSignalSummary;
  verificationSteps: GuideStep[];
  context: IngestContext;
  onValidate: () => void;
}) {
  const { t, i18n } = useTranslation('onboarding');
  const displayedSummary =
    validation.kind === 'success' || validation.kind === 'warning'
      ? validation.summary
      : passiveSummary;
  const health =
    validation.kind === 'success' || validation.kind === 'warning'
      ? validation.health
      : null;
  const status =
    validation.kind === 'success'
      ? 'success'
      : validation.kind === 'warning'
        ? 'warning'
        : validation.kind === 'error'
          ? 'error'
          : 'idle';

  return (
    <div className="min-w-0 overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
      <div
        className={cn(
          'flex flex-col gap-3 border-b px-4 py-3 sm:flex-row sm:items-center',
          status === 'success' && 'border-green/25 bg-green-dim',
          status === 'warning' && 'border-yellow/25 bg-yellow-dim',
          status === 'error' && 'border-red/25 bg-red-dim',
          status === 'idle' && 'border-bd-0',
        )}
      >
        <div className="min-w-0 flex-1">
          <div
            className={cn(
              'font-sans text-sm font-bold',
              status === 'success' && 'text-green-soft',
              status === 'warning' && 'text-yellow-soft',
              status === 'error' && 'text-red-soft',
              status === 'idle' && 'text-tx-0',
            )}
          >
            {t(`datasource_page.validation_state.${status}.title`)}
          </div>
          <div className="mt-0.5 font-sans text-xs leading-relaxed text-tx-2">
            {validation.kind === 'error'
              ? validation.message
              : t(`datasource_page.validation_state.${status}.description`, {
                  active: displayedSummary.activeSignals,
                  total: displayedSummary.expectedSignals,
                })}
          </div>
        </div>
        <ChromeButton
          variant={status === 'success' ? 'default' : 'primary'}
          disabled={
            validation.kind === 'pending' ||
            (context.isRum && (!context.applicationValid || !context.token))
          }
          onClick={onValidate}
        >
          {validation.kind === 'pending' ? (
            <LoaderCircle className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <RefreshCw className="h-3.5 w-3.5" />
          )}
          {validation.kind === 'pending'
            ? t('datasource_page.validating')
            : status === 'idle'
              ? t('datasource_page.verify_connection')
              : t('datasource_page.recheck')}
        </ChromeButton>
      </div>

      <dl className="grid divide-y divide-bd-0 sm:grid-cols-2 sm:divide-x sm:divide-y-0 xl:grid-cols-4">
        <ValidationMetric
          icon={Gauge}
          label={t('datasource_page.validation_metrics.receiver')}
          value={
            health?.ok
              ? t('datasource_page.receiver_healthy', { ms: health.latencyMs })
              : status === 'error'
                ? t('datasource_page.receiver_failed')
                : t('datasource_page.not_checked')
          }
          tone={health?.ok ? 'green' : status === 'error' ? 'red' : 'neutral'}
        />
        <ValidationMetric
          icon={Clock3}
          label={t('datasource_page.validation_metrics.last_received')}
          value={formatRelativeMicros(
            displayedSummary.lastReceivedAtMicros,
            i18n.language,
          )}
          tone={displayedSummary.lastReceivedAtMicros ? 'green' : 'neutral'}
        />
        <ValidationMetric
          icon={Workflow}
          label={t('datasource_page.validation_metrics.streams')}
          value={
            displayedSummary.streamNames.length > 0
              ? displayedSummary.streamNames.slice(0, 3).join(', ')
              : '—'
          }
        />
        <ValidationMetric
          icon={Database}
          label={t('datasource_page.validation_metrics.volume')}
          value={
            displayedSummary.rows > 0
              ? t('datasource_page.validation_volume', {
                  rate: formatEventRate(
                    displayedSummary.rows,
                    OVERVIEW_WINDOW_SECS,
                  ),
                  rows: formatCount(displayedSummary.rows),
                  bytes: formatBytes(displayedSummary.storedBytes),
                })
              : '—'
          }
        />
      </dl>

      {(status === 'warning' || status === 'error') && (
        <div className="border-t border-bd-0 px-4 py-3">
          <div className={uiLabelClass}>{t('datasource_page.possible_causes')}</div>
          <ul className="mt-2 space-y-1.5 pl-4 font-sans text-xs leading-relaxed text-tx-2">
            <li>{t('datasource_page.cause_collector')}</li>
            <li>{t('datasource_page.cause_token')}</li>
            <li>{t('datasource_page.cause_network')}</li>
          </ul>
          <div className="mt-3 flex flex-wrap items-center gap-2">
            <TestEventButton source={source} context={context} />
          </div>
        </div>
      )}

      {verificationSteps.length > 0 && (
        <div className="border-t border-bd-0 px-4 py-3">
          <div className={uiLabelClass}>{t('datasource_page.troubleshooting_command')}</div>
          <div className="mt-2 flex min-w-0 flex-col gap-3">
            {verificationSteps.map((step, index) => (
              <Step
                key={`${step.title}-${index}`}
                index={index + 1}
                step={step}
                context={context}
              />
            ))}
          </div>
        </div>
      )}

      <div className="border-t border-bd-0 px-4 py-2.5 font-sans text-xs leading-relaxed text-tx-3">
        {t('datasource_page.receiver_scope_note')}
      </div>
    </div>
  );
}

function ValidationMetric({
  icon: Icon,
  label,
  value,
  tone = 'neutral',
}: {
  icon: LucideIcon;
  label: string;
  value: string;
  tone?: 'neutral' | 'green' | 'red';
}) {
  return (
    <div className="min-w-0 px-4 py-3">
      <dt className="flex items-center gap-1.5 font-sans text-xs text-tx-3">
        <Icon className="h-3.5 w-3.5" />
        {label}
      </dt>
      <dd
        className={cn(
          'mt-1.5 truncate font-sans text-xs font-strong text-tx-1',
          tone === 'green' && 'text-green-soft',
          tone === 'red' && 'text-red-soft',
        )}
        title={value}
      >
        {value}
      </dd>
    </div>
  );
}

async function probeHealth(): Promise<HealthProbe> {
  const start = performance.now();
  try {
    const response = await fetch('/api/v1/healthz', { credentials: 'include' });
    const latencyMs = Math.round(performance.now() - start);
    return response.ok
      ? { ok: true, latencyMs }
      : { ok: false, latencyMs, message: `HTTP ${response.status}` };
  } catch (error) {
    return {
      ok: false,
      latencyMs: Math.round(performance.now() - start),
      message: (error as Error).message,
    };
  }
}

function TestEventButton({
  source,
  context,
}: {
  source: Source;
  context: IngestContext;
}) {
  const { t } = useTranslation('onboarding');
  const supportedSignals = source.signals.filter((signal) => signal !== 'profiles');
  const [state, setState] = React.useState<
    | { kind: 'idle' }
    | { kind: 'pending' }
    | { kind: 'ok'; accepted: number; rejected: number }
    | { kind: 'fail'; message: string }
  >({ kind: 'idle' });

  if (supportedSignals.length === 0) return null;

  const send = async () => {
    setState({ kind: 'pending' });
    try {
      const result = await sendTestEvent(source, context);
      setState({
        kind: 'ok',
        accepted: result.accepted,
        rejected: result.rejected,
      });
    } catch (error) {
      setState({ kind: 'fail', message: toApiError(error).message });
    }
  };

  return (
    <>
      <ChromeButton
        onClick={send}
        disabled={
          state.kind === 'pending' ||
          (context.isRum && (!context.applicationValid || !context.token))
        }
      >
        {state.kind === 'pending'
          ? t('datasource.test_event_sending')
          : t('datasource.test_event')}
      </ChromeButton>
      {state.kind === 'ok' && (
        <span className="font-sans text-xs font-semibold text-green-soft">
          {t('datasource.test_event_sent', {
            accepted: state.accepted,
            rejected: state.rejected,
          })}
        </span>
      )}
      {state.kind === 'fail' && (
        <span className="font-sans text-xs font-semibold text-red-soft">
          {state.message}
        </span>
      )}
    </>
  );
}

async function sendTestEvent(
  source: Source,
  context: IngestContext,
): Promise<ingestionApi.IngestResult> {
  if (source.rumPlatform) {
    return rumIngestApi.sendTestError({
      token: context.token,
      applicationId: context.applicationId,
      platform: source.rumPlatform,
      service: source.id,
    });
  }
  const signals = source.signals.filter(
    (signal): signal is Exclude<Signal, 'profiles'> => signal !== 'profiles',
  );
  const calls = signals.map((signal) => {
    if (signal === 'traces') {
      return ingestionApi.ingestTraces(DEFAULT_STREAM, [testTraceEvent(source)]);
    }
    if (signal === 'metrics') {
      return ingestionApi.ingestMetrics(DEFAULT_STREAM, [testMetricEvent(source)]);
    }
    return ingestionApi.ingestLogs(DEFAULT_STREAM, [testLogEvent(source)]);
  });
  const results = await Promise.all(calls);
  return results.reduce<ingestionApi.IngestResult>(
    (accumulator, result) => ({
      accepted: accumulator.accepted + result.accepted,
      rejected: accumulator.rejected + result.rejected,
      errors: [...(accumulator.errors ?? []), ...(result.errors ?? [])],
    }),
    { accepted: 0, rejected: 0, errors: [] },
  );
}

function testTraceEvent(source: Source): Record<string, unknown> {
  const startNs = Date.now() * 1_000_000;
  const endNs = startNs + 1_000_000;
  return {
    _timestamp: Math.floor(startNs / 1000),
    trace_id: crypto.randomUUID().replace(/-/g, ''),
    span_id: crypto.randomUUID().replace(/-/g, '').slice(0, 16),
    'service.name': source.id,
    name: 'molesignal.datasource.test',
    kind: 1,
    start_time_unix_nano: startNs,
    end_time_unix_nano: endNs,
    duration_ns: endNs - startNs,
    status_code: 'OK',
    generated_by: 'molesignal-web',
  };
}

function testMetricEvent(source: Source): Record<string, unknown> {
  return {
    name: 'molesignal_datasource_test_total',
    value: 1,
    timestamp: new Date().toISOString(),
    tags: { source: source.id, generated_by: 'molesignal-web' },
  };
}

function testLogEvent(source: Source): Record<string, unknown> {
  return {
    timestamp: new Date().toISOString(),
    level: 'info',
    message: `molesignal datasource test event from ${source.name}`,
    source: source.id,
    generated_by: 'molesignal-web',
  };
}

function rumSourceSummary(
  receipt: rumIngestApi.RumReceiptSummary = {
    rows: 0,
    lastReceivedAtMicros: null,
  },
): SourceSignalSummary {
  const active = receipt.rows > 0 && receipt.lastReceivedAtMicros != null;
  return {
    status: active ? 'healthy' : 'no_data',
    rows: receipt.rows,
    storedBytes: 0,
    lastReceivedAtMicros: receipt.lastReceivedAtMicros,
    activeSignals: active ? 1 : 0,
    expectedSignals: 1,
    streamNames: active ? ['rum_errors'] : [],
  };
}

function isVerificationStep(step: GuideStep): boolean {
  return /验证|verify|check/i.test(step.title);
}

function formatRelativeMicros(
  micros: number | null | undefined,
  locale: string,
): string {
  if (!micros) return '—';
  const deltaSeconds = Math.round(micros / 1000 - Date.now()) / 1000;
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  if (Math.abs(deltaSeconds) < 60) {
    return formatter.format(Math.round(deltaSeconds), 'second');
  }
  const minutes = deltaSeconds / 60;
  if (Math.abs(minutes) < 60) return formatter.format(Math.round(minutes), 'minute');
  const hours = minutes / 60;
  if (Math.abs(hours) < 24) return formatter.format(Math.round(hours), 'hour');
  return formatter.format(Math.round(hours / 24), 'day');
}

function formatBytes(value: number): string {
  if (value < 1024) return `${Math.round(value)} B`;
  if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KiB`;
  if (value < 1024 ** 3) return `${(value / 1024 ** 2).toFixed(1)} MiB`;
  return `${(value / 1024 ** 3).toFixed(2)} GiB`;
}

function formatCount(value: number): string {
  if (value < 1_000) return `${Math.round(value)}`;
  if (value < 1_000_000) return `${(value / 1_000).toFixed(1)}K`;
  return `${(value / 1_000_000).toFixed(1)}M`;
}

function formatEventRate(rows: number, windowSecs: number): string {
  if (rows <= 0 || windowSecs <= 0) return '—';
  const perSecond = rows / windowSecs;
  if (perSecond >= 1) return `${formatCount(perSecond)}/s`;
  const perMinute = perSecond * 60;
  if (perMinute >= 1) return `${formatCount(perMinute)}/min`;
  return `${formatCount(perMinute * 60)}/h`;
}

function viewPathForSource(source: Source): string {
  if (source.rumPlatform) return '/rum/errors';
  if (source.signals.includes('logs')) return '/logs';
  if (source.signals.includes('metrics')) return '/metrics';
  if (source.signals.includes('traces')) return '/traces';
  if (source.signals.includes('profiles')) return '/profiles';
  return '/streams';
}

function EmptyState() {
  const { t } = useTranslation('onboarding');
  return (
    <div className="grid h-full place-items-center px-6 text-center font-sans text-xs text-tx-2">
      <div>
        <Search className="mx-auto mb-3 h-6 w-6 text-tx-3" />
        {t('datasource_page.empty_select_source')}
      </div>
    </div>
  );
}
