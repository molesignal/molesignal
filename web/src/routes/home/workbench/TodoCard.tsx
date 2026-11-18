import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import {
  Bell,
  ChevronRight,
  ClipboardCheck,
  ListChecks,
  type LucideIcon,
  Sparkles,
  TriangleAlert,
  Waves,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as incidentsApi from '@/api/incidents';
import * as postmortemsApi from '@/api/postmortems';
import * as streamsApi from '@/api/streams';
import { toApiError } from '@/lib/http';
import { canAccessProductPath, hasPermission, useProductAccess } from '@/product/access';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { rowsOf } from '@/shell/notifications/model';
import { useAppNotifications } from '@/shell/notifications/useAppNotifications';
import { StatusWord, type StatusTone } from '@/shell/StatusWord';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';
import type { Severity } from '@/types/alerting';

import { buildTodos, type Todo } from './todos';
import { formatAge } from '../canvas/format';
import { CanvasSection } from '../canvas/layout';
import { summarizeFleet, toMicros } from '../canvas/streamCondition';

const DAY_MICROS = 24 * 60 * 60 * 1_000_000;
/** How far back incidents are read: as far as the bell and the palette look. */
const INCIDENT_WINDOW_SECS = 14 * 24 * 60 * 60;

const SEVERITY_TONE: Record<Severity, StatusTone> = {
  critical: 'red',
  error: 'red',
  warning: 'yellow',
  info: 'blue',
};

const ICON_INK: Partial<Record<StatusTone, string>> = {
  red: 'text-[var(--red-ink)]',
  yellow: 'text-[var(--yellow-ink)]',
  blue: 'text-[var(--blue-ink)]',
};

export const OPEN_INCIDENTS_QUERY_KEY = ['alerts', 'incidents'] as const;

/** The incidents of the workbench, read only for whoever can open incidents. */
export function useIncidents() {
  const access = useProductAccess();

  return useQuery({
    queryKey: OPEN_INCIDENTS_QUERY_KEY,
    queryFn: () => incidentsApi.list({ scope: 'all', windowSecs: INCIDENT_WINDOW_SECS }),
    enabled: canAccessProductPath('/alerts/incidents', access),
    refetchInterval: 60_000,
  });
}

/** What waits on the reader: the incidents, decisions, action items, streams and setting up. */
export function TodoCard({
  activation,
  onOpenActivation,
  className,
}: {
  activation: { remaining: number; nextLabel: string | null } | null;
  onOpenActivation: () => void;
  className?: string;
}) {
  const { t } = useTranslation('onboarding');
  const qc = useQueryClient();
  const access = useProductAccess();
  const userId = useAuthStore((state) => state.ctx?.userId ?? '');
  const notifications = useAppNotifications();
  const incidents = useIncidents();
  const acknowledgeAccess = useActionAccess({ permission: 'alerts.incidents.acknowledge' });
  const canReadStreams = canAccessProductPath('/streams', access);
  const actionItems = useQuery({
    queryKey: ['postmortems', 'actions', 'mine'],
    queryFn: postmortemsApi.myOpenActions,
    enabled: hasPermission('postmortems.read', access),
    refetchInterval: 5 * 60_000,
  });
  const runtime = useQuery({
    queryKey: ['streams', 'runtime', 'workbench'],
    queryFn: () => streamsApi.runtimeOverview({ windowSecs: 60 * 60, bucketCount: 12 }),
    enabled: canReadStreams,
    refetchInterval: 60_000,
  });
  const acknowledge = useMutation({
    mutationFn: incidentsApi.ack,
    onSuccess: () => void qc.invalidateQueries({ queryKey: OPEN_INCIDENTS_QUERY_KEY }),
    onError: (error) => toast.error(toApiError(error).message),
  });

  const nowMicros = Date.now() * 1000;
  const streamGaps = React.useMemo(() => {
    const overview = runtime.data;
    if (!overview) return 0;
    const fleet = summarizeFleet(
      overview.streams ?? [],
      overview.generatedAt != null ? toMicros(overview.generatedAt) : nowMicros,
    );
    return fleet.counts.interrupted + fleet.counts.stale;
    // The fleet is summed when the overview answers, not every minute the clock moves.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runtime.data]);

  const todos = buildTodos({
    userId,
    nowMicros,
    incidents: rowsOf(incidents.data),
    notifications,
    actionItems: rowsOf(actionItems.data),
    streamGaps,
    activation,
  });

  return (
    <CanvasSection
      title={
        <>
          {t('workbench.todos.title')}
          {todos.length > 0 && <span className="font-body tabular-nums text-tx-3">{todos.length}</span>}
        </>
      }
      className={className}
      bodyClassName="min-h-0 overflow-y-auto px-3 pb-2"
      testId="workbench-todos"
    >
      {incidents.isError && (
        <p role="status" className="flex items-center gap-1.5 px-2 pb-2 pt-1 font-sans text-xs text-tx-1">
          <TriangleAlert aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-[var(--orange-ink)]" />
          {t('workbench.todos.incidents_unavailable')}
        </p>
      )}
      {todos.length === 0 ? (
        !incidents.isError && (
          <p className="px-2 pb-3 pt-1 font-sans text-sm text-tx-3">
            {incidents.isLoading ? t('workbench.loading') : t('workbench.todos.empty')}
          </p>
        )
      ) : (
        <ul>
          {todos.map((todo) => (
            <TodoRow
              key={todo.key}
              todo={todo}
              nowMicros={nowMicros}
              canAcknowledge={acknowledgeAccess.allowed}
              acknowledging={acknowledge.isPending && acknowledge.variables === keyIncident(todo)}
              onAcknowledge={(id) => acknowledge.mutate(id)}
              onOpenActivation={onOpenActivation}
            />
          ))}
        </ul>
      )}
    </CanvasSection>
  );
}

function keyIncident(todo: Todo): string | undefined {
  return todo.kind === 'incident' ? todo.incident.id : undefined;
}

function TodoRow({
  todo,
  nowMicros,
  canAcknowledge,
  acknowledging,
  onAcknowledge,
  onOpenActivation,
}: {
  todo: Todo;
  nowMicros: number;
  /** Without the permission an incident row only opens the incident. */
  canAcknowledge: boolean;
  acknowledging: boolean;
  onAcknowledge: (incidentId: string) => void;
  onOpenActivation: () => void;
}) {
  const { t } = useTranslation('onboarding');
  const row = describe(todo, nowMicros, t);

  return (
    <li
      data-todo={todo.kind}
      data-home-surface={todo.kind === 'activation' ? 'activation' : undefined}
      className="group relative flex min-h-[52px] items-center gap-3 rounded-md px-2 py-1.5 transition-colors duration-fast hover:bg-bg-2"
    >
      <row.icon
        aria-hidden="true"
        className={cn('mt-0.5 h-4 w-4 shrink-0 self-start', (row.tone && ICON_INK[row.tone]) || 'text-tx-3')}
      />
      <div className="min-w-0 flex-1">
        {row.to ? (
          <Link to={row.to} className="block truncate font-sans text-sm font-strong text-tx-0 after:absolute after:inset-0">
            {row.title}
          </Link>
        ) : (
          <button
            type="button"
            onClick={onOpenActivation}
            className="block w-full truncate text-left font-sans text-sm font-strong text-tx-0 after:absolute after:inset-0"
          >
            {row.title}
          </button>
        )}
        <div className="mt-0.5 truncate font-sans text-xs text-tx-3">{row.meta}</div>
      </div>
      {todo.kind === 'incident' && canAcknowledge ? (
        <ChromeButton
          size="sm"
          className="relative z-10"
          disabled={acknowledging}
          onClick={() => onAcknowledge(todo.incident.id)}
        >
          {t('workbench.todos.acknowledge')}
        </ChromeButton>
      ) : (
        <ChevronRight
          aria-hidden="true"
          className="h-4 w-4 shrink-0 text-tx-3 transition-[color,transform] duration-fast group-hover:translate-x-0.5 group-hover:text-tx-1"
        />
      )}
    </li>
  );
}

type Translate = TFunction<'onboarding'>;

function joined(parts: readonly React.ReactNode[]): React.ReactNode {
  return parts.filter(Boolean).map((part, index) => (
    <React.Fragment key={index}>
      {index > 0 && <span aria-hidden="true"> · </span>}
      {part}
    </React.Fragment>
  ));
}

/** What a to-do says: its icon, its line, what is under it and where it leads. */
function describe(
  todo: Todo,
  nowMicros: number,
  t: Translate,
): { icon: LucideIcon; tone?: StatusTone; title: string; meta: React.ReactNode; to?: string } {
  switch (todo.kind) {
    case 'incident': {
      const { incident } = todo;
      const tone = SEVERITY_TONE[incident.severity];
      return {
        icon: Bell,
        tone,
        title: incident.summary || incident.id,
        meta: joined([
          <StatusWord key="severity" tone={tone} strong={tone === 'red'}>
            {t(`workbench.severity.${incident.severity}`)}
          </StatusWord>,
          incident.affectedServices[0],
          formatAge(incident.createdAt),
        ]),
        to: `/alerts/incidents/${encodeURIComponent(incident.id)}`,
      };
    }
    case 'approval':
      return {
        icon: ClipboardCheck,
        title: todo.title,
        meta: joined([t('workbench.todos.approval'), todo.description]),
        ...(todo.to ? { to: todo.to } : {}),
      };
    case 'action_item': {
      const { item } = todo;
      const due =
        item.dueAt === null
          ? null
          : todo.overdue
            ? (
                <StatusWord key="due" tone="red">
                  {t('workbench.todos.overdue', { count: Math.max(1, Math.floor((nowMicros - item.dueAt) / DAY_MICROS)) })}
                </StatusWord>
              )
            : t('workbench.todos.due_in', { count: Math.max(0, Math.ceil((item.dueAt - nowMicros) / DAY_MICROS)) });
      return {
        icon: ListChecks,
        title: item.title,
        meta: joined([t('workbench.todos.action_item', { title: item.postmortemTitle }), due]),
        to: `/alerts/incidents/${encodeURIComponent(item.incidentId)}`,
      };
    }
    case 'stream_gap':
      return {
        icon: Waves,
        tone: 'yellow',
        title: t('workbench.todos.stream_gap', { count: todo.count }),
        meta: t('workbench.todos.stream_gap_meta'),
        to: '/streams',
      };
    case 'activation':
      return {
        icon: Sparkles,
        tone: 'blue',
        title: t('workbench.todos.activation', { count: todo.remaining }),
        meta: todo.nextLabel ? t('workbench.todos.activation_next', { step: todo.nextLabel }) : '',
      };
  }
}
