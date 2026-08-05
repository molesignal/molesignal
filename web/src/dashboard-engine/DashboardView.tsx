import { useQuery } from '@tanstack/react-query';
import {
  ChevronDown,
  Download,
  Ellipsis,
  Maximize2,
  RefreshCw,
  Share2,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';

import * as dashboardsApi from '@/api/dashboards';
import * as foldersApi from '@/api/folders';
import {
  restrictActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { type ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { ResourceShareDialog } from '@/sharing/ResourceShareDialog';
import { ChromeButton, TimeRangeChip } from '@/shell/chrome';
import { FormDrawer } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import { setFullscreenDashboard } from '@/shell/wallboard';
import { useAuthStore } from '@/stores/auth';

import { DashboardRenderer } from './DashboardRenderer';
import { isDashboardEngineEnabled } from './featureFlag';
import {
  dashboardDefinitionFromApi,
  flattenElements,
  serializeDashboardDefinition,
} from './model';
import {
  parseIntervalMilliseconds,
  type DashboardRefreshCadence,
} from './refresh/policy';
import type { DashboardDefinition } from './schema';

export function DashboardView() {
  const { t, i18n } = useTranslation('dashboards');
  const { id } = useParams<{ id: string }>();
  const nav = useNavigate();
  const [searchParams] = useSearchParams();
  const reportRenderMode = searchParams.get('report_render') === '1';
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const shareAccess = useActionAccess({ permission: 'dashboards.share' });
  const editPermission = useActionAccess({ permission: 'dashboards.edit' });
  const [detailsOpen, setDetailsOpen] = React.useState(false);
  const [shareOpen, setShareOpen] = React.useState(false);
  const [refreshNonce, setRefreshNonce] = React.useState(0);
  const [refreshing, setRefreshing] = React.useState(false);
  const [refreshSelection, setRefreshSelection] = React.useState('off');
  const [rendererState, setRendererState] = React.useState<
    'loading' | 'ready'
  >('loading');
  const dashboardQuery = useQuery({
    queryKey: ['dashboards', 'get', id],
    queryFn: () => dashboardsApi.get(id!),
    enabled: Boolean(id),
  });
  const foldersQuery = useQuery({
    queryKey: ['folders', 'list'],
    queryFn: foldersApi.list,
  });
  const definitionResult = React.useMemo(
    () => {
      if (!dashboardQuery.data) {
        return { definition: null, error: null };
      }
      try {
        return {
          definition: dashboardDefinitionFromApi(dashboardQuery.data),
          error: null,
        };
      } catch (error) {
        return { definition: null, error };
      }
    },
    [dashboardQuery.data],
  );
  const definition = definitionResult.definition;
  const modelError = definitionResult.error;
  const editAccess = restrictActionAccess(
    editPermission,
    Boolean(definition?.editable),
    t('detail.read_only', { defaultValue: 'This dashboard is read-only.' }),
  );
  React.useEffect(() => {
    setRendererState('loading');
  }, [definition?.uid]);
  React.useEffect(() => {
    if (!definition) return;
    setRefreshSelection(refreshSettingValue(definition.refreshSettings));
  }, [definition]);
  const refreshInterval: DashboardRefreshCadence =
    refreshSelection === 'live'
      ? 'auto'
      : parseIntervalMilliseconds(refreshSelection);
  const folderName = definition?.folderId
    ? foldersQuery.data?.find((folder) => folder.id === definition.folderId)
        ?.name ?? definition.folderId
    : t('list.default_folder');
  const panelCount = definition
    ? flattenElements(definition.elements).filter(
        (element) => element.kind === 'panel',
      ).length
    : 0;
  const state: ProductStateProps | null = dashboardQuery.isLoading
    ? { variant: 'loading' }
    : dashboardQuery.isError
      ? { variant: 'error', error: dashboardQuery.error }
      : modelError
        ? { variant: 'error', error: modelError }
      : !definition
        ? { variant: 'empty', title: t('detail.not_found') }
        : null;
  React.useEffect(() => {
    if (!reportRenderMode) return;
    const root = document.documentElement;
    const nextState =
      dashboardQuery.isError || modelError
        ? 'error'
        : definition && rendererState === 'ready'
          ? 'ready'
          : 'loading';
    root.dataset.reportRenderState = nextState;
    if (nextState === 'error') {
      const error = dashboardQuery.error ?? modelError;
      root.dataset.reportRenderError =
        error instanceof Error ? error.message : String(error);
    } else {
      delete root.dataset.reportRenderError;
    }
    return () => {
      delete root.dataset.reportRenderState;
      delete root.dataset.reportRenderError;
    };
  }, [
    dashboardQuery.error,
    dashboardQuery.isError,
    definition,
    modelError,
    rendererState,
    reportRenderMode,
  ]);

  const exportDashboard = () => {
    if (!definition) return;
    const blob = new Blob([serializeDashboardDefinition(definition)], {
      type: 'application/json',
    });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = `${safeFilename(definition.title)}.json`;
    anchor.click();
    URL.revokeObjectURL(url);
  };

  if (!isDashboardEngineEnabled()) {
    return (
      <DetailPage
        title={definition?.title ?? t('detail.breadcrumb')}
        backTo="/dashboards"
        breadcrumbs={null}
      >
        <div className="grid min-h-[55vh] place-items-center rounded-md border border-dashed border-bd-1 bg-bg-1 p-8 font-sans text-sm text-tx-2">
          {t('detail.engine_disabled')}
        </div>
      </DetailPage>
    );
  }

  return (
    <>
      <DetailPage
        title={definition?.title ?? t('detail.breadcrumb')}
        subtitle={
          definition
            ? t('detail.summary', {
                folder: folderName,
                panels: t(
                  panelCount === 1
                    ? 'list.labels.panel_one'
                    : 'list.labels.panel_other',
                  { count: panelCount },
                ),
                updated: formatUpdated(definition.updatedAt),
              })
            : undefined
        }
        breadcrumbs={null}
        backTo="/dashboards"
        state={state}
        toolbar={
          definition ? (
            <>
              {!definition.timeSettings.hideTimePicker && <TimeRangeChip />}
              <div className="flex h-9 items-center overflow-hidden rounded-md border border-bd-1 bg-bg-2 text-tx-1 transition-colors hover:border-bd-2 hover:bg-bg-3">
                <button
                  type="button"
                  aria-label={t('detail.refresh')}
                  title={t('detail.refresh')}
                  onClick={() => {
                    setRefreshNonce((value) => value + 1);
                    toast.success(t('detail.refreshed'));
                  }}
                  className="grid h-full w-9 place-items-center text-tx-2 transition-colors hover:text-tx-0"
                >
                  <RefreshCw
                    className={cn(
                      'h-4 w-4',
                      refreshing && 'animate-spin',
                    )}
                  />
                </button>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <button
                      type="button"
                      aria-label={t('detail.refresh_menu')}
                      title={t('detail.refresh_menu')}
                      className="flex h-full min-w-[4.75rem] items-center justify-between gap-2 px-3 font-sans text-sm font-strong text-tx-1 transition-colors hover:text-tx-0"
                    >
                      <span>
                        {formatRefreshSetting(
                          refreshSelection,
                          i18n.resolvedLanguage ?? i18n.language,
                          t('detail.refresh_off'),
                          t('detail.refresh_live'),
                        )}
                      </span>
                      <ChevronDown className="h-3.5 w-3.5" />
                    </button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" className="min-w-44">
                    <DropdownMenuLabel>
                      {t('detail.refresh_mode')}
                    </DropdownMenuLabel>
                    <DropdownMenuSeparator />
                    <DropdownMenuRadioGroup
                      value={refreshSelection}
                      onValueChange={setRefreshSelection}
                    >
                      <DropdownMenuRadioItem value="off">
                        {t('detail.refresh_off')}
                      </DropdownMenuRadioItem>
                      <DropdownMenuRadioItem value="live">
                        {t('detail.refresh_live')}
                      </DropdownMenuRadioItem>
                      {definition.refreshSettings.allowedIntervals
                        .filter((value) => value !== 'off')
                        .map((value) => (
                          <DropdownMenuRadioItem key={value} value={value}>
                            {formatRefreshSetting(
                              value,
                              i18n.resolvedLanguage ?? i18n.language,
                              t('detail.refresh_off'),
                              t('detail.refresh_live'),
                            )}
                          </DropdownMenuRadioItem>
                        ))}
                    </DropdownMenuRadioGroup>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
              <ChromeButton
                disabled={shareAccess.disabled}
                disabledReason={shareAccess.reason}
                onClick={() => setShareOpen(true)}
              >
                <Share2 className="h-3 w-3" />
                {t('actions.share')}
              </ChromeButton>
              <ChromeButton
                onClick={() => {
                  if (!orgId || !definition.id) return;
                  setFullscreenDashboard(orgId, {
                    dashboardId: definition.id,
                    title: definition.title,
                    setAt: Date.now(),
                  });
                  toast.success(
                    t('detail.fullscreen_saved', {
                      title: definition.title,
                    }),
                  );
                }}
              >
                <Maximize2 className="h-3 w-3" />{' '}
                {t('actions.set_fullscreen')}
              </ChromeButton>
              <ChromeButton
                variant="primary"
                disabled={editAccess.disabled}
                disabledReason={editAccess.reason}
                onClick={() => nav(`/dashboards/${definition.id}/edit`)}
              >
                {t('actions.edit')}
              </ChromeButton>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <ChromeButton
                    aria-label={t('detail.more')}
                    title={t('detail.more')}
                  >
                    <Ellipsis className="h-4 w-4" />
                  </ChromeButton>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end">
                  <DropdownMenuItem onSelect={() => setDetailsOpen(true)}>
                    {t('detail.open_details')}
                  </DropdownMenuItem>
                  <DropdownMenuItem onSelect={exportDashboard}>
                    <Download className="h-3.5 w-3.5" />{' '}
                    {t('list.actions.export')}
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </>
          ) : null
        }
      >
        {definition && (
          <DashboardRenderer
            dashboard={definition}
            orgId={orgId}
            refreshNonce={refreshNonce}
            refreshIntervalOverride={refreshInterval}
            onRefreshStateChange={setRefreshing}
            {...(reportRenderMode
              ? { onRenderStateChange: setRendererState }
              : {})}
            {...(editAccess.allowed
              ? {
                  onEditPanel: (panelId: string) =>
                    nav(`/dashboards/${definition.id}/edit?panel=${panelId}`),
                }
              : {})}
          />
        )}
      </DetailPage>
      {definition && (
        <FormDrawer
          open={detailsOpen}
          onOpenChange={setDetailsOpen}
          title={t('detail.details_title')}
          subtitle={definition.title}
          width={480}
        >
          <dl className="divide-y divide-bd-0 border-y border-bd-0">
            {[
              [t('detail.metadata.folder'), folderName],
              [t('detail.metadata.panels'), String(panelCount)],
              [
                t('detail.metadata.elements'),
                String(flattenElements(definition.elements).length),
              ],
              [t('detail.metadata.schema'), String(definition.schemaVersion)],
              [t('detail.metadata.updated'), formatUpdated(definition.updatedAt)],
              [t('detail.metadata.owner'), definition.updatedBy],
              [t('detail.metadata.dashboard_id'), definition.id],
              [t('detail.metadata.uid'), definition.uid],
            ].map(([label, value]) => (
              <div
                key={label}
                className="grid grid-cols-[132px_minmax(0,1fr)] gap-4 py-3 font-sans text-sm"
              >
                <dt className="text-tx-3">{label}</dt>
                <dd className="min-w-0 break-all font-mono text-xs text-tx-0">
                  {value}
                </dd>
              </div>
            ))}
          </dl>
        </FormDrawer>
      )}
      {definition && (
        <ResourceShareDialog
          open={shareOpen}
          onOpenChange={setShareOpen}
          resourceType="dashboard"
          resourceId={definition.id}
          title={definition.title}
          resourceTags={definition.tags}
          variableNames={definition.variables.map((variable) => variable.name)}
        />
      )}
    </>
  );
}

function safeFilename(value: string): string {
  return value.replace(/[^\p{L}\p{N}._-]+/gu, '-') || 'dashboard';
}

function formatUpdated(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function refreshSettingValue(
  settings: DashboardDefinition['refreshSettings'],
): string {
  if (!settings.enabled || settings.mode === 'off') return 'off';
  if (settings.mode === 'live') return 'live';
  return settings.defaultInterval || 'off';
}

function formatRefreshSetting(
  value: string,
  language: string,
  offLabel: string,
  liveLabel: string,
): string {
  if (value === 'off') return offLabel;
  if (value === 'live') return liveLabel;
  if (!language.toLowerCase().startsWith('zh')) return value;
  const match = /^(\d+)(ms|s|m|h|d)$/.exec(value);
  if (!match) return value;
  const unit = ({
    ms: '毫秒',
    s: '秒',
    m: '分钟',
    h: '小时',
    d: '天',
  } as Record<string, string>)[match[2] ?? ''];
  return `${match[1]} ${unit}`;
}
