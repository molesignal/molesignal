import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { History, MoreHorizontal, Play, Plus, RotateCcw, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as drillsApi from '@/api/agent/drills';
import type { DrillSummary } from '@/api/agent/drills';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { IconButton } from '@/shell/chrome';
import { Button } from '@/shell/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import { formatRelativeMicros } from '@/time/relative';

import { DrillRuns } from './DrillRuns';
import { canRun, failureNote, runGoing, runStatusLabel, runnable } from './drillsModel';
import { NewDrill } from './NewDrill';
import { ScoreWord } from './ScoreWord';
import { DRILLS_KEY, useScoringSettings } from './state';
import { EmptyTab, ModulePage } from '../operations/Pages';

const HEADER = 'whitespace-nowrap';
/** While a run is under way the list reads itself again this often. */
const POLL_MS = 10_000;

/**
 * Drills: resolved incidents Mole Agent investigates again as if they were
 * happening, graded against the root cause people established. Running them
 * all after a change of model, prompt or tools shows whether investigations
 * got better.
 */
export function QualityDrills() {
  const { t, i18n } = useTranslation('agent');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const queryClient = useQueryClient();
  const { threshold } = useScoringSettings();
  const [creating, setCreating] = React.useState(false);
  const [viewing, setViewing] = React.useState<DrillSummary | null>(null);
  const [deleting, setDeleting] = React.useState<DrillSummary | null>(null);
  const drills = useQuery({
    queryKey: DRILLS_KEY,
    queryFn: drillsApi.listDrills,
    retry: false,
    refetchInterval: (query) =>
      query.state.data?.some((drill) => runGoing(drill.lastRun)) ? POLL_MS : false,
  });

  const refresh = () => void queryClient.invalidateQueries({ queryKey: DRILLS_KEY });
  const failed = (error: unknown) => toast.error(toApiError(error).message);

  const runOne = useMutation({
    mutationFn: (drill: DrillSummary) => drillsApi.runDrill(drill.id),
    onSuccess: (run) => {
      if (run.status === 'failed') toast.error(failureNote(t, run));
      else toast.success(t('quality.drills.started'));
      refresh();
    },
    onError: failed,
  });
  const runAll = useMutation({
    mutationFn: () => drillsApi.runAll(),
    onSuccess: (result) => {
      toast.success(t('quality.drills.started_all', { count: result.runs.length }));
      refresh();
    },
    onError: failed,
  });
  const remove = useMutation({
    mutationFn: (drill: DrillSummary) => drillsApi.deleteDrill(drill.id),
    onSuccess: () => {
      toast.success(t('quality.drills.deleted'));
      setDeleting(null);
      refresh();
    },
    onError: failed,
  });

  const rows = drills.data ?? [];
  const nowMicros = (drills.dataUpdatedAt || Date.now()) * 1000;
  const create = (
    <Button size="sm" onClick={() => setCreating(true)}>
      <Plus />
      {t('quality.drills.new')}
    </Button>
  );
  const actions = (
    <div className="flex items-center gap-2">
      {rows.length > 0 && (
        <Button
          size="sm"
          variant="ghost"
          disabled={runnable(rows).length === 0 || runAll.isPending}
          onClick={() => runAll.mutate()}
        >
          <RotateCcw />
          {t('quality.drills.run_all')}
        </Button>
      )}
      {create}
    </div>
  );

  return (
    <ModulePage
      title={t('quality.views.drills')}
      description={t('quality.drills.description')}
      action={actions}
      flush={rows.length > 0}
    >
      {drills.isPending ? (
        <ProductState variant="loading" />
      ) : drills.isError ? (
        <ProductState variant="error" error={drills.error} />
      ) : rows.length === 0 ? (
        <EmptyTab title={t('quality.drills.none')} hint={t('quality.drills.none_hint')} />
      ) : (
        <div className="overflow-x-auto">
          <DataTable
            rows={rows}
            rowKey={(row) => row.id}
            onRowClick={setViewing}
            emptyLabel={t('quality.drills.none')}
            className="min-w-[820px] rounded-none border-0 bg-transparent"
            columns={[
              {
                key: 'drill',
                header: t('quality.drills.columns.drill'),
                className: 'w-full max-w-0',
                headerClassName: HEADER,
                cell: (row) => (
                  <div data-quality-drill={row.id} className="min-w-0 py-1">
                    <div className="truncate text-tx-0" title={row.title}>
                      {row.title}
                    </div>
                    <div className="mt-0.5 truncate text-xs text-tx-2" title={row.reference}>
                      {row.reference}
                    </div>
                  </div>
                ),
              },
              {
                key: 'source',
                header: t('quality.drills.columns.source'),
                width: 108,
                headerClassName: HEADER,
                cell: (row) => <span className="text-xs text-tx-1">{t(`quality.drills.sources.${row.source}`)}</span>,
              },
              {
                key: 'last',
                header: t('quality.drills.columns.last_run'),
                width: 168,
                headerClassName: HEADER,
                cell: (row) => <LastRun drill={row} threshold={threshold} locale={locale} nowMicros={nowMicros} />,
              },
              {
                key: 'mean',
                header: t('quality.drills.columns.mean'),
                width: 72,
                headerClassName: `${HEADER} text-right`,
                className: 'text-right',
                cell: (row) => <ScoreWord score={row.meanScore} threshold={threshold} />,
              },
              {
                key: 'runs',
                header: t('quality.drills.columns.runs'),
                width: 64,
                headerClassName: `${HEADER} text-right`,
                className: 'text-right tabular-nums',
                cell: (row) => row.runs.toLocaleString(),
              },
              {
                key: 'actions',
                header: <span className="sr-only">{t('quality.columns.actions')}</span>,
                width: 104,
                className: 'overflow-visible text-right',
                cell: (row) => (
                  <div className="flex items-center justify-end gap-[2px]">
                    <IconButton
                      aria-label={t('quality.drills.run_named', { name: row.title })}
                      title={t('quality.drills.run')}
                      disabled={!canRun(row) || runOne.isPending}
                      onClick={(event) => {
                        event.stopPropagation();
                        runOne.mutate(row);
                      }}
                    >
                      <Play className="h-3.5 w-3.5" />
                    </IconButton>
                    <IconButton
                      aria-label={t('quality.drills.runs_named', { name: row.title })}
                      title={t('quality.drills.runs')}
                      onClick={(event) => {
                        event.stopPropagation();
                        setViewing(row);
                      }}
                    >
                      <History className="h-3.5 w-3.5" />
                    </IconButton>
                    <DropdownMenu>
                      <DropdownMenuTrigger asChild>
                        <IconButton
                          aria-label={t('quality.drills.more_named', { name: row.title })}
                          onClick={(event) => event.stopPropagation()}
                        >
                          <MoreHorizontal className="h-3.5 w-3.5" />
                        </IconButton>
                      </DropdownMenuTrigger>
                      <DropdownMenuContent align="end" className="w-40">
                        <DropdownMenuItem onSelect={() => setDeleting(row)} className="text-[var(--red-ink)]">
                          <Trash2 className="h-3.5 w-3.5" />
                          {t('quality.drills.delete')}
                        </DropdownMenuItem>
                      </DropdownMenuContent>
                    </DropdownMenu>
                  </div>
                ),
              },
            ]}
          />
        </div>
      )}
      {creating && <NewDrill onClose={() => setCreating(false)} />}
      <DrillRuns drill={viewing} threshold={threshold} onClose={() => setViewing(null)} />
      <ConfirmDialog
        open={deleting !== null}
        onOpenChange={(open) => !open && setDeleting(null)}
        title={t('quality.drills.delete_confirm.title', { name: deleting?.title ?? '' })}
        description={t('quality.drills.delete_confirm.description')}
        confirmLabel={t('quality.drills.delete')}
        cancelLabel={t('actions.cancel', { ns: 'common' })}
        busyLabel={t('quality.drills.delete_confirm.busy')}
        destructive
        busy={remove.isPending}
        onConfirm={() => deleting && remove.mutate(deleting)}
      />
    </ModulePage>
  );
}

/** How the last run went: under way, its score, or why it has none. */
function LastRun({
  drill,
  threshold,
  locale,
  nowMicros,
}: {
  drill: DrillSummary;
  threshold: number;
  locale: string;
  nowMicros: number;
}) {
  const { t } = useTranslation('agent');
  const run = drill.lastRun;

  if (!run) return <span className="text-xs text-tx-3">{t('quality.drills.never_run')}</span>;

  const when = (
    <span className="text-tx-3" title={formatMicrosActive(run.createdAt)}>
      {formatRelativeMicros(run.createdAt, locale, nowMicros)}
    </span>
  );

  return (
    <div data-quality-drill-last={run.status} className="flex min-w-0 items-baseline gap-2 text-xs">
      {run.status === 'done' ? (
        <ScoreWord score={run.score} threshold={threshold} />
      ) : run.status === 'failed' ? (
        <span className="truncate text-[var(--red-ink)]" title={failureNote(t, run)}>
          {failureNote(t, run)}
        </span>
      ) : (
        <span className="text-[var(--blue-ink)]">{runStatusLabel(t, run)}</span>
      )}
      {when}
    </div>
  );
}
