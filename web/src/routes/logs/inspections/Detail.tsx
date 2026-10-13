import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { Pencil, Play } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useParams, useSearchParams } from 'react-router-dom';

import * as inspectionsApi from '@/api/logInspections';
import type { LogInspection, LogInspectionRun } from '@/api/logInspections';
import { writeClipboardText } from '@/lib/clipboard';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { useProductAccess } from '@/product/access';
import type { ProductStateProps } from '@/product/states';
import { DetailPage } from '@/product/templates';
import { ChromeButton } from '@/shell/chrome';
import { QueryState } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';
import { Switch } from '@/shell/ui/switch';

import { canManageInspections, canReadInspections } from './access';
import { INSPECTIONS_KEY, InspectionEditor } from './Editor';
import {
  DEFAULT_LIMITS,
  durationLabel,
  isActiveRun,
  refreshInterval,
  runAsName,
  scheduleLabel,
} from './model';
import { RunList } from './RunList';
import { RunReport } from './RunReport';

/** How many runs a page of the history holds, and the most the backend lists at once. */
const RUNS_PAGE = 50;
const RUNS_MAX = 200;

const SURFACE =
  'min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]';

/**
 * One scheduled inspection: what it does, said once at the head, then its
 * runs beside the report of the one that is picked. A link from a
 * notification names the run in the address, and that run is the one shown.
 */
export function LogInspectionDetail() {
  const { t } = useTranslation('logs');
  const { id = '' } = useParams<{ id: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const queryClient = useQueryClient();
  const access = useProductAccess();
  const readable = canReadInspections(access);
  const manageable = canManageInspections(access);
  const [editing, setEditing] = React.useState(false);
  const [runsLimit, setRunsLimit] = React.useState(RUNS_PAGE);
  const [copiedRun, setCopiedRun] = React.useState<string | null>(null);

  // The inspection, its limits and its runs are read on one clock: a row that says "running"
  // beside a head that says it has finished is worse than either a moment late.
  const query = useQuery({
    queryKey: [...INSPECTIONS_KEY, 'detail', id, runsLimit],
    queryFn: async () => {
      const [inspection, runs, limits] = await Promise.all([
        inspectionsApi.get(id),
        inspectionsApi.listRuns(id, { limit: runsLimit }),
        inspectionsApi.limits(),
      ]);

      return { inspection, runs, limits };
    },
    enabled: readable && id !== '',
    staleTime: 0,
    placeholderData: keepPreviousData,
    refetchOnWindowFocus: true,
    refetchInterval: (current) => {
      const data = current.state.data;
      const active =
        Boolean(data?.inspection.active_run) ||
        Boolean(data?.runs.some((run) => isActiveRun(run.status)));

      return refreshInterval(active);
    },
  });
  const inspection = query.data?.inspection;
  const runs = query.data?.runs ?? [];
  const selectedId = searchParams.get('run') ?? runs[0]?.id ?? null;
  const listed = runs.find((run) => run.id === selectedId);

  // The list carries no report: the run that is picked is read whole, and again while it is
  // under way or once the list says it has changed.
  const report = useQuery({
    queryKey: [...INSPECTIONS_KEY, 'run', id, selectedId, listed?.status ?? 'unlisted'],
    queryFn: () => inspectionsApi.getRun(id, selectedId!),
    enabled: readable && Boolean(selectedId) && Boolean(inspection),
    staleTime: 0,
    retry: false,
  });

  const select = (picked: LogInspectionRun) =>
    setSearchParams(
      (current) => {
        const params = new URLSearchParams(current);
        params.set('run', picked.id);
        return params;
      },
      { replace: true },
    );
  const refresh = () => queryClient.invalidateQueries({ queryKey: INSPECTIONS_KEY });
  const onError = (error: unknown) => toast.error(toApiError(error).message);
  const run = useMutation({
    mutationFn: () => inspectionsApi.runNow(id),
    onSuccess: (queued) => {
      toast.success(t('inspections.toast.queued'));
      select(queued);
      return refresh();
    },
    onError,
  });
  const toggle = useMutation({
    mutationFn: (enabled: boolean) => inspectionsApi.setEnabled(id, enabled),
    onSuccess: (saved) => {
      toast.success(t(saved.enabled ? 'inspections.toast.enabled' : 'inspections.toast.paused'));
      return refresh();
    },
    onError,
  });

  const copyReport = async (picked: LogInspectionRun) => {
    try {
      await writeClipboardText(picked.report ?? '');
      setCopiedRun(picked.id);
    } catch {
      // Clipboard access can be denied by the browser.
    }
  };

  const deniedReason = manageable ? undefined : t('inspections.denied.manage');
  const state: ProductStateProps | null = !readable
    ? {
        variant: 'permission-denied',
        title: t('inspections.denied.title'),
        description: t('inspections.denied.description'),
      }
    : query.isError
      ? { variant: 'error', error: query.error }
      : !inspection
        ? { variant: 'loading' }
        : null;
  const nowMicros = Date.now() * 1000;

  return (
    <>
      <DetailPage
        appearance="surface"
        title={inspection?.name ?? t('inspections.title')}
        subtitle={inspection ? statement(t, inspection) : undefined}
        breadcrumbs={[
          { labelKey: 'logs', to: '/logs' },
          { labelKey: 'breadcrumbs.logs_inspections', to: '/logs/inspections' },
          inspection
            ? { labelKey: 'breadcrumbs.logs_inspection_detail', label: inspection.name }
            : { labelKey: 'breadcrumbs.logs_inspection_detail' },
        ]}
        backTo="/logs/inspections"
        state={state}
        toolbar={
          inspection ? (
            <>
              <label className="flex h-9 items-center gap-2 font-sans text-xs text-tx-1">
                <Switch
                  checked={inspection.enabled}
                  disabled={Boolean(deniedReason) || toggle.isPending}
                  disabledReason={deniedReason}
                  aria-label={t('inspections.actions.toggle_named', { name: inspection.name })}
                  onCheckedChange={(enabled) => toggle.mutate(enabled)}
                />
                <span>{t(inspection.enabled ? 'inspections.enabled' : 'inspections.cells.paused')}</span>
              </label>
              <ChromeButton
                disabled={Boolean(deniedReason) || run.isPending || Boolean(inspection.active_run)}
                disabledReason={
                  deniedReason ??
                  (inspection.active_run ? t('inspections.actions.run_active') : undefined)
                }
                onClick={() => run.mutate()}
                data-log-inspection-run-now
              >
                <Play className="h-3.5 w-3.5" aria-hidden="true" />
                {t('inspections.actions.run')}
              </ChromeButton>
              <ChromeButton
                disabled={Boolean(deniedReason)}
                disabledReason={deniedReason}
                onClick={() => setEditing(true)}
              >
                <Pencil className="h-3.5 w-3.5" aria-hidden="true" />
                {t('inspections.actions.edit')}
              </ChromeButton>
            </>
          ) : undefined
        }
      >
        {inspection && runs.length === 0 ? (
          <div className={SURFACE}>
            <QueryState
              state="empty"
              emptyLabel={t(
                inspection.enabled
                  ? 'inspections.detail.no_runs'
                  : 'inspections.detail.no_runs_paused',
              )}
              className="min-h-40"
            />
          </div>
        ) : inspection ? (
          <div className="grid min-w-0 items-start gap-[12px] lg:grid-cols-[340px_minmax(0,1fr)]">
            <div className={SURFACE}>
              <RunList
                runs={runs}
                selectedId={selectedId}
                nowMicros={nowMicros}
                more={runs.length >= runsLimit && runsLimit < RUNS_MAX}
                onSelect={select}
                onMore={() => setRunsLimit((limit) => Math.min(RUNS_MAX, limit + RUNS_PAGE))}
              />
            </div>
            <div className={SURFACE}>
              {report.data ? (
                <RunReport
                  inspection={inspection}
                  run={report.data}
                  nowMicros={nowMicros}
                  copied={copiedRun === report.data.id}
                  onCopy={() => void copyReport(report.data)}
                />
              ) : report.isError ? (
                <QueryState
                  state="error"
                  error={report.error}
                  errorLabel={t('inspections.detail.run_missing')}
                  className="min-h-40"
                />
              ) : (
                <QueryState
                  state="loading"
                  loadingLabel={t('inspections.detail.loading_report')}
                  className="min-h-40"
                />
              )}
            </div>
          </div>
        ) : null}
      </DetailPage>

      <InspectionEditor
        open={editing}
        onOpenChange={setEditing}
        inspection={inspection ?? null}
        limits={query.data?.limits ?? DEFAULT_LIMITS}
      />
    </>
  );
}

/** What the inspection does, in one line: how often, of what, as whom, and when next. */
function statement(t: TFunction<'logs'>, inspection: LogInspection): string {
  const parts = [
    scheduleLabel(t, inspection.schedule),
    inspection.stream,
    t('inspections.cells.window', { duration: durationLabel(t, inspection.window_secs) }),
    t('inspections.detail.run_as', { name: runAsName(inspection) }),
    inspection.enabled && inspection.next_run_at_micros
      ? t('inspections.detail.next_run', {
          time: formatMicrosActive(inspection.next_run_at_micros, false),
        })
      : t('inspections.cells.paused'),
  ];

  return parts.join(' · ');
}
