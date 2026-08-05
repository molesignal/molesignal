import { useMutation, useQuery } from '@tanstack/react-query';
import {
  AlertTriangle,
  Download,
  Eye,
  KeyRound,
  Loader2,
  LockKeyhole,
  ShieldCheck,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as resourceSharesApi from '@/api/resourceShares';
import {
  DashboardRenderer,
  type DashboardPanelQueryExecutor,
} from '@/dashboard-engine/DashboardRenderer';
import { queryResultToDataFrames } from '@/dashboard-engine/dataframe';
import { dashboardDefinitionFromStoredModel } from '@/dashboard-engine/model';
import type { DashboardDefinition } from '@/dashboard-engine/schema';
import { toApiError } from '@/lib/http';
import { ChromeButton, Pill } from '@/shell/chrome';
import { Input } from '@/shell/ui/input';
import { useTimeStore } from '@/stores/useTimeStore';

export function PublicShare() {
  const { t } = useTranslation('common');
  const metadataQuery = useQuery({
    queryKey: ['public-resource-share'],
    queryFn: resourceSharesApi.publicMetadata,
    retry: false,
    refetchOnWindowFocus: false,
  });

  if (metadataQuery.isLoading) {
    return <PublicState icon={<Loader2 className="h-5 w-5 animate-spin" />} />;
  }
  if (metadataQuery.isError || !metadataQuery.data) {
    return (
      <PublicState
        icon={<AlertTriangle className="h-5 w-5" />}
        title={t('sharing.public.unavailable_title')}
        description={
          metadataQuery.error
            ? toApiError(metadataQuery.error).message
            : t('sharing.public.unavailable_hint')
        }
      />
    );
  }
  if (metadataQuery.data.requires_password) {
    return (
      <PasswordGate
        onUnlocked={() => void metadataQuery.refetch()}
      />
    );
  }
  return metadataQuery.data.kind === 'dashboard' ? (
    <PublicDashboard metadata={metadataQuery.data} />
  ) : (
    <PublicReport metadata={metadataQuery.data} />
  );
}

function PublicDashboard({
  metadata,
}: {
  metadata: resourceSharesApi.PublicShareMetadata;
}) {
  const { t } = useTranslation('common');
  const constraints = metadata.constraints ?? {};
  const maxTimeRangeSecs = positiveNumber(
    constraints.max_time_range_secs,
    3600,
  );
  const allowTimeChanges = constraints.allow_time_range_changes === true;
  const allowVariableChanges =
    constraints.allow_variable_changes === true;
  const setWindow = useTimeStore((state) => state.setWindow);
  const [rangeSecs, setRangeSecs] = React.useState(
    Math.min(3600, maxTimeRangeSecs),
  );
  const dashboard = React.useMemo<DashboardDefinition | null>(() => {
    try {
      return dashboardDefinitionFromStoredModel(
        metadata.definition,
        metadata.title ?? t('sharing.public.dashboard'),
        `public-${metadata.watermark?.share_id ?? 'share'}`,
      );
    } catch {
      return null;
    }
  }, [
    metadata.definition,
    metadata.title,
    metadata.watermark?.share_id,
    t,
  ]);
  React.useEffect(() => {
    setWindow({
      from: relativeExpression(rangeSecs),
      to: 'now',
      mode: 'relative',
    });
  }, [rangeSecs, setWindow]);

  const executeQuery = React.useCallback<DashboardPanelQueryExecutor>(
    async (panelId, query, context) => {
      const result = await resourceSharesApi.runPublicPanelQuery({
        panel_id: panelId,
        ref_id: query.refId,
        from_micros: context.timeRange.from,
        to_micros: context.timeRange.to,
        variables: context.variables,
      });
      return queryResultToDataFrames(
        result,
        query.refId,
        query.dataSourceType,
        query.legend,
      );
    },
    [],
  );

  if (!dashboard) {
    return (
      <PublicState
        icon={<AlertTriangle className="h-5 w-5" />}
        title={t('sharing.public.invalid_dashboard')}
        description={t('sharing.public.invalid_dashboard_hint')}
      />
    );
  }

  return (
    <div className="min-h-screen bg-bg-0 text-tx-1">
      <PublicHeader
        title={metadata.title ?? dashboard.title}
        expiresAt={metadata.expires_at_micros}
        badge={t('sharing.public.read_only')}
      >
        {allowTimeChanges && (
          <label className="flex items-center gap-2 text-xs text-tx-3">
            <span>{t('sharing.public.time_range')}</span>
            <select
              aria-label={t('sharing.public.time_range')}
              value={rangeSecs}
              onChange={(event) => setRangeSecs(Number(event.target.value))}
              className="h-9 rounded-md border border-bd-1 bg-bg-1 px-2 text-xs text-tx-1"
            >
              {publicRangeOptions(maxTimeRangeSecs).map((option) => (
                <option key={option.seconds} value={option.seconds}>
                  {option.key
                    ? t(`sharing.public.ranges.${option.key}`)
                    : t('sharing.public.ranges.seconds', {
                        count: option.seconds,
                      })}
                </option>
              ))}
            </select>
          </label>
        )}
      </PublicHeader>
      <main className="mx-auto w-full max-w-[1680px] p-3 sm:p-5">
        <DashboardRenderer
          dashboard={dashboard}
          orgId="public-share"
          restricted
          refreshIntervalOverride={false}
          variableControlsEnabled={allowVariableChanges}
          resolveVariableQueries={false}
          maxTimeRangeMicros={maxTimeRangeSecs * 1_000_000}
          panelQueryExecutor={executeQuery}
        />
      </main>
      <PublicWatermark metadata={metadata} />
    </div>
  );
}

function PublicReport({
  metadata,
}: {
  metadata: resourceSharesApi.PublicShareMetadata;
}) {
  const { t } = useTranslation('common');
  const fileUrl = resourceSharesApi.publicFileUrl();
  const isImage = metadata.content_type?.startsWith('image/');
  return (
    <div className="flex min-h-screen flex-col bg-bg-0 text-tx-1">
      <PublicHeader
        title={metadata.title ?? t('sharing.public.report')}
        expiresAt={metadata.expires_at_micros}
        badge={t('sharing.public.snapshot')}
      >
        {metadata.allow_download && (
          <ChromeButton
            variant="primary"
            onClick={() =>
              window.location.assign(resourceSharesApi.publicFileUrl(true))
            }
          >
            <Download className="h-4 w-4" />
            {t('sharing.public.download')}
          </ChromeButton>
        )}
      </PublicHeader>
      <main className="min-h-0 flex-1 p-3 sm:p-5">
        <div className="mx-auto h-[calc(100vh-112px)] max-w-[1440px] overflow-hidden rounded-lg border border-bd-0 bg-white shadow-sm">
          {isImage ? (
            <div className="grid h-full place-items-center overflow-auto bg-bg-2 p-4">
              <img
                src={fileUrl}
                alt={metadata.title ?? t('sharing.public.report')}
                className="max-h-full max-w-full object-contain"
              />
            </div>
          ) : (
            <iframe
              title={metadata.title ?? t('sharing.public.report')}
              src={fileUrl}
              className="h-full w-full border-0"
              referrerPolicy="no-referrer"
            />
          )}
        </div>
      </main>
      <PublicWatermark metadata={metadata} />
    </div>
  );
}

function PasswordGate({ onUnlocked }: { onUnlocked: () => void }) {
  const { t } = useTranslation('common');
  const [password, setPassword] = React.useState('');
  const unlockMutation = useMutation({
    mutationFn: () => resourceSharesApi.unlock(password),
    onSuccess: onUnlocked,
  });
  return (
    <PublicState
      icon={<LockKeyhole className="h-5 w-5" />}
      title={t('sharing.public.password_title')}
      description={t('sharing.public.password_hint')}
    >
      <form
        className="mt-5 grid w-full gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          unlockMutation.mutate();
        }}
      >
        <label className="relative">
          <KeyRound className="pointer-events-none absolute left-3 top-3 h-4 w-4 text-tx-3" />
          <Input
            autoFocus
            type="password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            placeholder={t('sharing.public.password_placeholder')}
            className="h-10 pl-10"
          />
        </label>
        {unlockMutation.isError && (
          <p role="alert" className="text-left text-xs text-danger">
            {t('sharing.public.password_invalid')}
          </p>
        )}
        <ChromeButton
          type="submit"
          variant="primary"
          disabled={!password || unlockMutation.isPending}
          className="h-10 justify-center"
        >
          {unlockMutation.isPending && (
            <Loader2 className="h-4 w-4 animate-spin" />
          )}
          {t('sharing.public.continue')}
        </ChromeButton>
      </form>
    </PublicState>
  );
}

function PublicHeader({
  title,
  expiresAt,
  badge,
  children,
}: {
  title: string;
  expiresAt?: number | null | undefined;
  badge: string;
  children?: React.ReactNode;
}) {
  const { t } = useTranslation('common');
  return (
    <header className="sticky top-0 z-20 flex min-h-14 flex-wrap items-center gap-3 border-b border-bd-0 bg-bg-1/95 px-4 py-2 backdrop-blur sm:px-6">
      <div className="flex min-w-0 items-center gap-2">
        <span className="text-lg leading-none text-indigo">⌁</span>
        <span className="font-sans text-sm font-semibold text-tx-0">
          MoleSignal
        </span>
        <span className="h-4 w-px bg-bd-1" />
        <h1 className="min-w-0 truncate text-sm font-medium text-tx-1">
          {title}
        </h1>
      </div>
      <Pill tone="neutral" className="gap-1">
        <Eye className="h-3 w-3" />
        {badge}
      </Pill>
      <div className="ml-auto flex items-center gap-3">
        {expiresAt && (
          <span className="hidden text-xs text-tx-3 md:inline">
            {t('sharing.public.expires_at', {
              value: new Date(expiresAt / 1000).toLocaleString(),
            })}
          </span>
        )}
        {children}
      </div>
    </header>
  );
}

function PublicWatermark({
  metadata,
}: {
  metadata: resourceSharesApi.PublicShareMetadata;
}) {
  const { t } = useTranslation('common');
  if (!metadata.watermark) return null;
  return (
    <div className="pointer-events-none fixed bottom-2 right-3 z-30 rounded border border-bd-0 bg-bg-1/80 px-2 py-1 font-mono text-type-micro text-tx-3 backdrop-blur">
      {t('sharing.public.watermark', {
        value: new Date(
          metadata.watermark.accessed_at_micros / 1000,
        ).toLocaleString(),
      })}
    </div>
  );
}

function PublicState({
  icon,
  title,
  description,
  children,
}: {
  icon: React.ReactNode;
  title?: string;
  description?: string;
  children?: React.ReactNode;
}) {
  const { t } = useTranslation('common');
  return (
    <main className="grid min-h-screen place-items-center bg-bg-0 p-5">
      <section className="w-full max-w-sm rounded-xl border border-bd-0 bg-bg-1 p-6 text-center shadow-sm">
        <div className="mx-auto grid h-11 w-11 place-items-center rounded-lg border border-indigo/20 bg-indigo-dim text-indigo-soft">
          {icon}
        </div>
        <h1 className="mt-4 text-base font-semibold text-tx-0">
          {title ?? t('sharing.public.loading_title')}
        </h1>
        <p className="mt-2 text-sm leading-6 text-tx-3">
          {description ?? t('sharing.public.loading_hint')}
        </p>
        {children}
        <div className="mt-5 flex items-center justify-center gap-1.5 text-xs text-tx-3">
          <ShieldCheck className="h-3.5 w-3.5" />
          {t('sharing.public.trust_label')}
        </div>
      </section>
    </main>
  );
}

function positiveNumber(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) && value > 0
    ? value
    : fallback;
}

function relativeExpression(seconds: number): string {
  if (seconds % 86_400 === 0) return `now-${seconds / 86_400}d`;
  if (seconds % 3600 === 0) return `now-${seconds / 3600}h`;
  if (seconds % 60 === 0) return `now-${seconds / 60}m`;
  return `now-${seconds}s`;
}

function publicRangeOptions(maxSeconds: number) {
  const candidates = [
    { seconds: 15 * 60, key: 'fifteen_minutes' },
    { seconds: 60 * 60, key: 'one_hour' },
    { seconds: 6 * 60 * 60, key: 'six_hours' },
    { seconds: 24 * 60 * 60, key: 'twenty_four_hours' },
    { seconds: 7 * 24 * 60 * 60, key: 'seven_days' },
  ].filter((option) => option.seconds <= maxSeconds);
  return candidates.length > 0
    ? candidates
    : [{ seconds: maxSeconds, key: null }];
}
