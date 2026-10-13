import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Pause, Pencil, Play, Upload } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate, useParams } from 'react-router-dom';

import * as syntheticsApi from '@/api/synthetics';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import type { ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { ChromeButton } from '@/shell/chrome';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';
import { toast } from '@/shell/ui/sonner';

import { CheckConfiguration } from './Configuration';
import { CheckRevisions, CheckRuns } from './Lists';
import { CheckOverview } from './Overview';
import { SYNTHETICS_REFRESH_MILLIS } from '../../data';
import { useLoadState } from '../../Frame';
import { activeRevision, checkTarget } from '../../model';
import { ResultDrawer } from '../../results/Drawer';

const TABS = ['overview', 'results', 'configuration', 'revisions'] as const;
type Tab = (typeof TABS)[number];

function tabOf(pathname: string): Tab {
  if (pathname.includes('/configuration')) return 'configuration';
  if (pathname.includes('/revisions')) return 'revisions';
  if (pathname.includes('/results')) return 'results';
  return 'overview';
}

/**
 * One check on a page of its own.
 *
 * The page is the check's: its name is the title, and the tabs under it are
 * views of this check, not of the module. What can be done to the check is in
 * the header, with the step that moves it forward as the one filled button:
 * run it, or, for a draft, publish it.
 */
export function CheckDetail() {
  const { t } = useTranslation('synthetics');
  const { monitorId, resultId } = useParams();
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'synthetics.manage' });
  const detailQuery = useQuery({
    queryKey: ['synthetics', 'monitor', monitorId],
    queryFn: () => syntheticsApi.getMonitor(monitorId!),
    enabled: Boolean(monitorId),
    refetchInterval: SYNTHETICS_REFRESH_MILLIS,
  });
  const resultQuery = useQuery({
    queryKey: ['synthetics', 'monitor', monitorId, 'results', 100],
    queryFn: () => syntheticsApi.listResults(monitorId!, { limit: 100 }),
    enabled: Boolean(monitorId),
    refetchInterval: SYNTHETICS_REFRESH_MILLIS,
  });
  const locationsQuery = useQuery({
    queryKey: ['synthetics', 'locations'],
    queryFn: syntheticsApi.listLocations,
  });
  const detail = detailQuery.data;
  const monitor = detail?.monitor;
  const revision = activeRevision(detail);
  const results = resultQuery.data ?? [];
  const locations = locationsQuery.data ?? [];
  const tab = tabOf(location.pathname);
  const selectedResult = results.find((result) => result.id === resultId);
  const selectedRevision = detail?.revisions.find(
    (item) => item.id === selectedResult?.monitor_revision_id,
  );
  const draft = Boolean(monitor?.draft_revision_id && monitor.draft_revision_id === revision?.id);
  const heartbeat = revision?.spec.kind === 'heartbeat';
  // A draft is published once a test of it has passed; a heartbeat has nothing to test.
  const publishable = Boolean(revision && (heartbeat || revision.last_test_passed_at));

  const refresh = () => queryClient.invalidateQueries({ queryKey: ['synthetics'] });
  const run = useMutation({
    mutationFn: () =>
      draft
        ? syntheticsApi.testRevision(monitorId!, revision!.id)
        : syntheticsApi.runMonitor(monitorId!),
    onSuccess: () => toast.success(t('checks.run_queued')),
    onError: (error) => toast.error(toApiError(error).message),
  });
  const publish = useMutation({
    mutationFn: () => syntheticsApi.publishRevision(monitorId!, revision!.id),
    onSuccess: async () => {
      toast.success(t('checks.published'));
      await refresh();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const lifecycle = useMutation({
    mutationFn: () =>
      monitor?.lifecycle === 'paused'
        ? syntheticsApi.resumeMonitor(monitorId!)
        : syntheticsApi.pauseMonitor(monitorId!),
    onSuccess: refresh,
    onError: (error) => toast.error(toApiError(error).message),
  });

  const loadState = useLoadState({
    pending: detailQuery.isPending || resultQuery.isPending || locationsQuery.isPending,
    error: detailQuery.error ?? resultQuery.error ?? locationsQuery.error,
    onRetry: () => {
      void detailQuery.refetch();
      void resultQuery.refetch();
      void locationsQuery.refetch();
    },
  });
  const state: ProductStateProps | null =
    loadState ?? (revision ? null : { variant: 'empty', title: t('detail.no_revision') });
  const base = `/synthetics/checks/${encodeURIComponent(monitorId ?? '')}`;

  return (
    <>
      <DetailPage
        appearance="surface"
        title={monitor?.name ?? t('tabs.checks')}
        subtitle={
          monitor
            ? [t(`kinds.${monitor.kind}`), checkTarget(revision?.spec)].filter(Boolean).join(' · ')
            : undefined
        }
        state={state}
        subnav={
          monitor && revision ? (
            <ModuleTabs
              label={t('detail.views_label')}
              className="relative z-10"
              dataAttributes={{ 'data-testid': 'synthetics-check-tabs' }}
              items={TABS.map((item) => ({
                key: item,
                to: item === 'overview' ? base : `${base}/${item}`,
                active: tab === item,
                label: t(`detail.${item}`),
              }))}
            />
          ) : undefined
        }
        toolbar={
          monitor && revision ? (
            <>
              {monitor.active_revision_id && (
                <ChromeButton
                  onClick={() => lifecycle.mutate()}
                  disabled={manageAccess.disabled || lifecycle.isPending}
                  disabledReason={manageAccess.reason}
                >
                  {monitor.lifecycle === 'paused' ? (
                    <Play aria-hidden className="h-3.5 w-3.5" />
                  ) : (
                    <Pause aria-hidden className="h-3.5 w-3.5" />
                  )}
                  {t(monitor.lifecycle === 'paused' ? 'actions.resume' : 'actions.pause')}
                </ChromeButton>
              )}
              <ChromeButton
                onClick={() => navigate(`${base}/edit`)}
                disabled={manageAccess.disabled}
                disabledReason={manageAccess.reason}
              >
                <Pencil aria-hidden className="h-3.5 w-3.5" />
                {t('actions.edit')}
              </ChromeButton>
              {!heartbeat && (
                <ChromeButton
                  variant={draft ? 'default' : 'primary'}
                  onClick={() => run.mutate()}
                  disabled={
                    manageAccess.disabled ||
                    run.isPending ||
                    (!draft && monitor.lifecycle !== 'active')
                  }
                  disabledReason={manageAccess.reason}
                >
                  <Play aria-hidden className="h-3.5 w-3.5" />
                  {t(draft ? 'actions.test_revision' : 'actions.run_now')}
                </ChromeButton>
              )}
              {draft && (
                <ChromeButton
                  variant="primary"
                  onClick={() => publish.mutate()}
                  disabled={manageAccess.disabled || !publishable || publish.isPending}
                  disabledReason={
                    manageAccess.reason ?? (!publishable ? t('checks.publish_requires_test') : undefined)
                  }
                >
                  <Upload aria-hidden className="h-3.5 w-3.5" />
                  {t('actions.publish')}
                </ChromeButton>
              )}
            </>
          ) : undefined
        }
      >
        {monitor && revision && detail && (
          <>
            {tab === 'overview' && (
              <CheckOverview
                monitor={monitor}
                revision={revision}
                results={results}
                locations={locations}
                draft={draft}
              />
            )}
            {tab === 'results' && (
              <CheckRuns
                results={results}
                locations={locations}
                onOpen={(result) => navigate(`${base}/results/${encodeURIComponent(result.id)}`)}
              />
            )}
            {tab === 'configuration' && (
              <CheckConfiguration revision={revision} locations={locations} />
            )}
            {tab === 'revisions' && (
              <CheckRevisions
                revisions={detail.revisions}
                activeId={monitor.active_revision_id}
                draftId={monitor.draft_revision_id}
              />
            )}
          </>
        )}
      </DetailPage>
      <ResultDrawer
        result={selectedResult}
        monitor={monitor}
        revision={selectedRevision}
        locations={locations}
        onClose={() => navigate(`${base}/results`, { replace: true })}
      />
    </>
  );
}
