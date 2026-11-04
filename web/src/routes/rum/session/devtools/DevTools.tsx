import { useQuery } from '@tanstack/react-query';
import { RefreshCw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import type { RumComment, TimelineItem } from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { Skeleton } from '@/shell/ui/skeleton';
import { useAuthStore } from '@/stores/auth';

import { useToolsState } from './address';
import { CommentList } from './CommentList';
import { ErrorList } from './ErrorList';
import { LogList } from './LogList';
import { axisOf, defaultTab, inView, lastBefore, SHOWS, shown, toolRows, type ToolTab, type ViewRow, viewAt } from './model';
import { NetworkList } from './NetworkList';
import { PerformanceList } from './PerformanceList';
import { formatClock } from '../../replay/clock';
import { type ReplayController, replayClockMicros, useReplaySelector } from '../../replay/controller';
import { ViewWaterfall } from '../../waterfall/Waterfall';

const MINUTE_MICROS = 60 * 1_000_000;

const CONTROL = 'h-7 border-0 bg-[var(--control-surface)] px-2.5 text-xs hover:bg-bg-3 focus-visible:bg-bg-3 data-[state=open]:bg-bg-3';

/**
 * The tools under the replay: what went wrong, what the page asked for, and how each page did, from the
 * events of the session itself. They follow the replay: the row it is at is lit and kept in sight, and a
 * row moves it to that moment. The errors are the session's, in order; the requests are those of one view,
 * the one the replay is in until another is chosen, and can be shown as the waterfall of its load. The logs
 * are what the page and the application wrote, and the comments what people said about moments of it.
 */
export function DevTools({
  controller,
  items,
  truncated,
  application,
  sessionStart,
  hasReplay,
  language,
  session,
  comments,
  commentsKey,
}: {
  controller: ReplayController;
  items: readonly TimelineItem[];
  /** The kinds of rows the timeline had more of than it carries. */
  truncated: readonly string[];
  application: string;
  /** When the session began, in microseconds: the ruler's start where there is no replay. */
  sessionStart: number;
  hasReplay: boolean;
  language: string;
  session: string;
  comments: readonly RumComment[];
  /** What the page read the comments under. */
  commentsKey: readonly unknown[];
}) {
  const { t } = useTranslation('rum');
  const { view: state, set } = useToolsState();
  const rows = React.useMemo(() => toolRows(items), [items]);

  // The ruler is the replay's where there is one, so that a time here is the time on its progress bar.
  const origin = useReplaySelector(controller, (replay) => replay.origin ?? -1);
  const zero = origin >= 0 ? origin * 1_000 : sessionStart;
  const followed = useReplaySelector(controller, (replay) => viewAt(rows.views, replayClockMicros(replay))?.id ?? '');
  const litView = useReplaySelector(controller, (replay) => {
    const clock = replayClockMicros(replay);

    return clock === null ? -1 : Math.max(0, lastBefore(rows.views, clock));
  });

  // The rows are of the view the replay is in until one is chosen; a session with no replay has none to follow.
  const chosen = state.view !== null && state.view !== 'all' && rows.views.some((view) => view.id === state.view) ? state.view : null;
  const scope: string | null = state.view === 'all' ? null : (chosen ?? (hasReplay ? followed || null : null));
  const scoped = scope === null ? null : (rows.views.find((view) => view.id === scope) ?? null);

  const network = React.useMemo(() => inView(rows.network, scope), [rows.network, scope]);
  // The ruler of the bars is that of every request of the view, whichever are shown.
  const axis = React.useMemo(() => (scoped ? axisOf(network, scoped.at) : null), [network, scoped]);
  const listed = React.useMemo(() => shown(network, state.show), [network, state.show]);

  const tab = state.tab ?? defaultTab(rows);
  const display = tab === 'network' && scoped ? state.display : 'list';

  const noun = (type: string) => t(`explorer.types.${type}`);
  const cut = (type: string) => (truncated.includes(type) ? t('session_detail.timeline_truncated', { types: noun(type) }) : undefined);

  const viewLabel = (view: ViewRow) => `${view.name} · ${formatClock((view.at - zero) / 1_000)}`;

  return (
    <section data-rum-devtools="" data-rum-tool-tab={tab} aria-label={t('session_detail.tools.aria')} className="overflow-hidden rounded-lg border border-bd-1 bg-bg-1">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-bd-0 px-3 py-2">
        <ViewTabs<ToolTab>
          label={t('session_detail.tools.tabs_aria')}
          value={tab}
          onValueChange={(key) => set({ tab: key })}
          dataAttributes={{ 'data-rum-tool-tabs': '' }}
          items={[
            { key: 'errors', label: t('session_detail.tools.tabs.errors'), count: rows.errors.length },
            { key: 'network', label: t('session_detail.tools.tabs.network'), count: network.length },
            { key: 'performance', label: t('session_detail.tools.tabs.performance'), count: rows.views.length },
            { key: 'logs', label: t('session_detail.tools.tabs.logs'), count: rows.logs.length },
            { key: 'comments', label: t('session_detail.tools.tabs.comments'), count: comments.length },
          ]}
        />
        {tab === 'network' && rows.views.length > 0 && (
          <span className="ml-auto flex flex-wrap items-center gap-2">
            <FittedSelect
              aria-label={t('session_detail.tools.view')}
              data-rum-tool-view=""
              value={scope ?? 'all'}
              onValueChange={(value) => set({ view: value === 'follow' ? null : value })}
              options={[
                ...(hasReplay && state.view !== null ? [{ value: 'follow', label: t('session_detail.tools.follow') }] : []),
                { value: 'all', label: t('session_detail.tools.all_views') },
                ...rows.views.map((view) => ({ value: view.id, label: viewLabel(view) })),
              ]}
              maxWidth={320}
              prefix={t('session_detail.tools.view')}
              className={cn(CONTROL, 'font-strong text-tx-1')}
            />
            {display === 'list' && (
              <FittedSelect
                aria-label={t('waterfall.filters.show')}
                data-rum-tool-show=""
                value={state.show}
                onValueChange={(value) => set({ show: value })}
                options={SHOWS.map((show) => ({ value: show, label: t(`waterfall.filters.shows.${show}`) }))}
                maxWidth={240}
                prefix={t('waterfall.filters.show')}
                className={cn(CONTROL, 'font-strong text-tx-1')}
              />
            )}
            {scoped && (
              <ViewTabs
                label={t('session_detail.tools.display.label')}
                value={display}
                onValueChange={(key) => set({ display: key })}
                dataAttributes={{ 'data-rum-tool-display': '' }}
                items={[
                  { key: 'list', label: t('session_detail.tools.display.list') },
                  { key: 'waterfall', label: t('session_detail.tools.display.waterfall') },
                ]}
              />
            )}
          </span>
        )}
      </div>

      {tab === 'errors' && (
        <ErrorList rows={rows.errors} application={application} controller={controller} zero={zero} empty={t('session_detail.tools.errors.empty')} notice={cut('errors')} />
      )}
      {tab === 'network' &&
        (display === 'waterfall' && scoped ? (
          <WaterfallTool view={scoped} />
        ) : (
          <NetworkList
            rows={listed}
            axis={axis}
            controller={controller}
            zero={zero}
            empty={t(
              network.length > 0 ? 'session_detail.tools.network.no_match' : scope === null ? 'session_detail.tools.network.empty_all' : 'session_detail.tools.network.empty',
            )}
            notice={cut('resources')}
          />
        ))}
      {tab === 'performance' && (
        <PerformanceList
          rows={rows.views}
          lit={litView}
          language={language}
          empty={t('session_detail.tools.performance.empty')}
          onOpen={(view) => set({ tab: 'network', view: view.id, display: 'waterfall' })}
        />
      )}
      {tab === 'logs' && (
        <LogList rows={rows.logs} controller={controller} zero={zero} empty={t('session_detail.tools.logs.empty')} notice={cut('logs')} />
      )}
      {tab === 'comments' && (
        <CommentList session={session} application={application} comments={comments} controller={controller} zero={zero} queryKey={commentsKey} />
      )}
    </section>
  );
}

/** How one view loaded, as the waterfall of the explorer tells it: the view's own row is read for the moments of its load. */
function WaterfallTool({ view }: { view: ViewRow }) {
  const { t } = useTranslation('rum');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const row = useQuery({
    queryKey: ['rum', 'session', orgId, 'view', view.id],
    queryFn: ({ signal }) => rumApi.entity('views', view.id, { from: view.at - MINUTE_MICROS, to: view.at + MINUTE_MICROS }, signal),
    enabled: Boolean(orgId),
    staleTime: 60_000,
  });

  if (row.isPending) {
    return (
      <div role="status" aria-label={t('explorer.drawer.loading')} className="flex flex-col gap-2 px-4 py-4">
        <Skeleton className="h-3 w-2/3" />
        <Skeleton className="h-3 w-1/2" />
        <Skeleton className="h-3 w-3/5" />
      </div>
    );
  }

  if (row.isError) {
    return (
      <div className="flex items-center justify-between gap-3 px-4 py-4 font-sans text-xs text-tx-2">
        <span>{t('waterfall.failed')}</span>
        <button
          type="button"
          onClick={() => void row.refetch()}
          className="inline-flex h-7 items-center gap-1.5 rounded-md border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
        >
          <RefreshCw aria-hidden className="h-3.5 w-3.5" />
          {t('overview.retry')}
        </button>
      </div>
    );
  }

  return (
    <div data-rum-tool-waterfall="" className="p-3">
      <ViewWaterfall key={view.id} row={row.data} />
    </div>
  );
}
