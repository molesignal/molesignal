import { useQuery } from '@tanstack/react-query';
import {
  Bell,
  ChevronRight,
  Clock3,
  Server,
  UserRoundCheck,
  Waypoints,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as homeApi from '@/api/home';
import type { HomeHealthStatus } from '@/api/home';
import * as incidentsApi from '@/api/incidents';
import * as schedulesApi from '@/api/schedules';
import { cn } from '@/shell/lib/cn';
import { useUsers } from '@/shell/useUsers';
import { useAuthStore } from '@/stores/auth';

import { buildOnCallSnapshot } from './model';
import type { ChatContext } from './types';

type ConnectionState =
  | 'connected'
  | 'attention'
  | 'no_data'
  | 'checking'
  | 'unavailable';

const CONNECTION_DOT: Record<ConnectionState, string> = {
  connected: 'bg-green',
  attention: 'bg-orange',
  no_data: 'bg-tx-4',
  checking: 'bg-indigo',
  unavailable: 'bg-red',
};

function connectionForSignal(
  status: HomeHealthStatus | undefined,
  pending: boolean,
  error: boolean,
): ConnectionState {
  if (pending) return 'checking';
  if (error) return 'unavailable';
  if (status === 'healthy') return 'connected';
  if (status === 'degraded' || status === 'delayed') return 'attention';
  return 'no_data';
}

export interface SystemContextProps {
  context: ChatContext;
  rangePreset: string;
  variant?: 'sidebar' | 'drawer';
}

export function SystemContext({
  context,
  rangePreset,
  variant = 'sidebar',
}: SystemContextProps) {
  const { t } = useTranslation('agent');
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const users = useUsers();
  const [nowMicros, setNowMicros] = React.useState(() => Date.now() * 1_000);

  React.useEffect(() => {
    const timer = window.setInterval(
      () => setNowMicros(Date.now() * 1_000),
      60_000,
    );
    return () => window.clearInterval(timer);
  }, []);

  const homeOverview = useQuery({
    queryKey: ['home', 'overview', orgId, 3_600, 12],
    queryFn: () => homeApi.overview({ windowSecs: 3_600, bucketCount: 12 }),
    enabled: Boolean(orgId),
    retry: false,
    staleTime: 30_000,
  });
  const incidents = useQuery({
    queryKey: ['alerts', 'incidents', 'active', 7 * 86_400],
    queryFn: () =>
      incidentsApi.list({ scope: 'active', window_secs: 7 * 86_400 }),
    enabled: Boolean(orgId),
    retry: false,
    staleTime: 30_000,
  });
  const schedules = useQuery({
    queryKey: ['schedules'],
    queryFn: schedulesApi.list,
    enabled: Boolean(orgId),
    retry: false,
    staleTime: 60_000,
  });

  const incidentRows = Array.isArray(incidents.data) ? incidents.data : [];
  const scheduleRows = Array.isArray(schedules.data) ? schedules.data : [];
  const onCall = buildOnCallSnapshot(scheduleRows, nowMicros);
  const onCallName = onCall?.primaryUserId
    ? users.byId.get(onCall.primaryUserId)?.name ?? onCall.primaryUserId
    : t('workspace.unassigned');
  const signalStatus = (type: 'logs' | 'metrics' | 'traces') =>
    homeOverview.data?.signals?.find((signal) => signal.stream_type === type)
      ?.status;
  const connections = (['metrics', 'logs', 'traces'] as const).map((key) => ({
    key,
    state: connectionForSignal(
      signalStatus(key),
      homeOverview.isPending,
      homeOverview.isError,
    ),
  }));

  const content = (
    <>
      <header className="border-b border-bd-0 px-4 py-3.5">
        <h2 className="type-section-title font-display-strong text-tx-0">
          {t('workspace.context_title')}
        </h2>
        <p className="mt-0.5 type-micro text-tx-3">
          {t('workspace.context_description')}
        </p>
      </header>

      <div className="min-h-0 flex-1 overflow-auto">
        <section className="border-b border-bd-0 px-4 py-4">
          <h3 className="type-micro font-strong uppercase tracking-[0.08em] text-tx-3">
            {t('workspace.scope')}
          </h3>
          <dl className="mt-3 space-y-3">
            <ScopeRow
              icon={Server}
              label={t('context.service')}
              value={context.service || t('workspace.context_auto')}
            />
            <ScopeRow
              icon={Waypoints}
              label={t('context.environment')}
              value={context.environment || t('workspace.all_environments')}
            />
            <ScopeRow
              icon={Clock3}
              label={t('workspace.time_range')}
              value={t(`range.${rangePreset}`)}
            />
          </dl>
        </section>

        <section className="border-b border-bd-0 px-4 py-4">
          <h3 className="type-micro font-strong uppercase tracking-[0.08em] text-tx-3">
            {t('workspace.signals')}
          </h3>
          <div className="mt-2.5 flex flex-wrap gap-2">
            {connections.map((connection) => (
              <span
                key={connection.key}
                className="inline-flex items-center gap-1.5 rounded-full bg-bg-2 px-2.5 py-1 type-micro text-tx-2"
              >
                <span
                  aria-label={t(`workspace.connection_state.${connection.state}`)}
                  className={cn(
                    'h-1.5 w-1.5 rounded-full',
                    CONNECTION_DOT[connection.state],
                  )}
                />
                {t(`workspace.connections.${connection.key}`)}
              </span>
            ))}
          </div>
        </section>

        <ContextLink
          icon={Bell}
          label={t('workspace.active_alerts')}
          value={incidents.isPending ? '…' : incidents.isError ? '—' : incidentRows.length}
          to="/alerts/incidents"
          attention={incidentRows.length > 0}
        />
        <ContextLink
          icon={UserRoundCheck}
          label={t('workspace.on_call')}
          value={schedules.isPending ? '…' : schedules.isError ? '—' : onCallName}
          to={
            onCall
              ? `/alerts/schedules/${encodeURIComponent(onCall.scheduleId)}`
              : '/alerts/schedules'
          }
        />
      </div>
    </>
  );

  if (variant === 'drawer') {
    return <div className="flex h-full min-h-0 flex-col bg-bg-1">{content}</div>;
  }

  return (
    <aside
      data-testid="conversation-context"
      aria-label={t('workspace.context_title')}
      className="hidden w-[260px] shrink-0 flex-col border-l border-bd-0 bg-bg-1 xl:flex"
    >
      {content}
    </aside>
  );
}

function ScopeRow({
  icon: Icon,
  label,
  value,
}: {
  icon: typeof Server;
  label: string;
  value: string;
}) {
  return (
    <div className="flex items-start gap-2.5">
      <Icon aria-hidden="true" className="mt-0.5 h-3.5 w-3.5 shrink-0 text-tx-3" />
      <div className="min-w-0 flex-1">
        <dt className="type-micro text-tx-3">{label}</dt>
        <dd className="mt-0.5 truncate type-caption font-strong text-tx-1">{value}</dd>
      </div>
    </div>
  );
}

function ContextLink({
  icon: Icon,
  label,
  value,
  to,
  attention = false,
}: {
  icon: typeof Bell;
  label: string;
  value: number | string;
  to: string;
  attention?: boolean;
}) {
  return (
    <Link
      to={to}
      className="group flex min-h-[52px] items-center gap-2.5 border-b border-bd-0 px-4 py-3 transition-colors duration-fast hover:bg-bg-2 focus-visible:bg-bg-2"
    >
      <Icon aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-tx-3" />
      <span className="min-w-0 flex-1 type-caption text-tx-2">{label}</span>
      <span
        className={cn(
          'max-w-[112px] truncate font-mono type-label font-display-strong text-tx-0',
          attention && 'text-orange-soft',
        )}
      >
        {value}
      </span>
      <ChevronRight
        aria-hidden="true"
        className="h-3.5 w-3.5 shrink-0 text-tx-4 group-hover:text-indigo group-focus-visible:text-indigo"
      />
    </Link>
  );
}
