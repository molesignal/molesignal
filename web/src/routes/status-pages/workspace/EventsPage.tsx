import { useQuery } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as statusPagesApi from '@/api/statusPages';
import type {
  StatusPageEventView,
  StatusPageIncidentKind,
} from '@/api/statusPages';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import { PageBody } from '@/shell/PageHeader';

import {
  IMPACT_TONE,
  INCIDENT_TONE,
  affectedComponentNames,
  impactLabel,
  incidentStatusLabel,
} from '../model';
import { StatusPageEventDrawer } from './EventDrawer';
import { useStatusPageWorkspace } from './Layout';

const VIEWS: Record<StatusPageIncidentKind, StatusPageEventView[]> = {
  incident: ['current', 'draft', 'resolved'],
  maintenance: ['upcoming', 'in_progress', 'draft', 'completed'],
};

export function StatusPageEvents({ kind }: { kind: StatusPageIncidentKind }) {
  const { t } = useTranslation('status-pages');
  const navigate = useNavigate();
  const location = useLocation();
  const { eventId } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const { pageId, snapshot, manageAccess } = useStatusPageWorkspace();
  const requestedView = searchParams.get('view') as StatusPageEventView | null;
  const defaultView: StatusPageEventView = kind === 'incident' ? 'current' : 'upcoming';
  const view = requestedView && VIEWS[kind].includes(requestedView)
    ? requestedView
    : defaultView;
  const query = useQuery({
    queryKey: ['status-pages', pageId, 'events', kind, view],
    queryFn: () => statusPagesApi.listEvents(pageId, kind, view),
  });
  const canMutate = manageAccess.allowed && snapshot.page.lifecycle === 'active';
  const basePath = `/status-pages/${pageId}/${kind === 'incident' ? 'incidents' : 'maintenance'}`;
  const closeDrawer = () => navigate({ pathname: basePath, search: location.search }, { replace: true });

  return (
    <PageBody className="space-y-4">
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        <div className="flex min-w-0 items-center gap-1 overflow-x-auto">
          {VIEWS[kind].map((item) => (
            <ChromeButton
              key={item}
              size="sm"
              variant={view === item ? 'primary' : 'ghost'}
              onClick={() =>
                setSearchParams(item === defaultView ? {} : { view: item }, { replace: true })
              }
            >
              {t(`event_views.${item}`)}
              {query.data?.view === item && (
                <span className="type-micro font-mono opacity-70">{query.data.total}</span>
              )}
            </ChromeButton>
          ))}
        </div>
        <ChromeButton
          variant="primary"
          className="ml-auto"
          disabled={!canMutate}
          disabledReason={manageAccess.reason}
          onClick={() => navigate({ pathname: `${basePath}/new`, search: location.search })}
        >
          <Plus className="h-3.5 w-3.5" />
          {t(kind === 'incident' ? 'actions.new_incident' : 'actions.schedule_maintenance')}
        </ChromeButton>
      </div>

      {query.isLoading ? (
        <ProductState variant="loading" />
      ) : query.isError ? (
        <ProductState variant="error" error={query.error} />
      ) : (
        <div className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
          <DataTable
            rows={query.data?.items ?? []}
            rowKey={(event) => event.id}
            emptyLabel={t('states.no_events_in_view')}
            onRowClick={(event) =>
              navigate({ pathname: `${basePath}/${event.id}`, search: location.search })
            }
            columns={[
              {
                key: 'title',
                header: t('columns.title'),
                cell: (event) => (
                  <div className="min-w-0">
                    <div className="truncate text-tx-0">
                      {event.title || t('values.untitled_draft')}
                    </div>
                    <div className="mt-0.5 truncate text-xs font-normal text-tx-3">
                      {affectedComponentNames(event, snapshot.components).join(', ') || t('values.no_components')}
                    </div>
                  </div>
                ),
              },
              {
                key: 'status',
                header: t('columns.status'),
                width: 150,
                cell: (event) => (
                  <Pill tone={INCIDENT_TONE[event.status]}>
                    {event.publication_state === 'draft'
                      ? t('publication_state.draft')
                      : incidentStatusLabel(t, event.status)}
                  </Pill>
                ),
              },
              {
                key: 'impact',
                header: t('columns.impact'),
                width: 140,
                cell: (event) => (
                  <Pill tone={IMPACT_TONE[event.impact]}>{impactLabel(t, event.impact)}</Pill>
                ),
              },
              {
                key: 'started',
                header: t(kind === 'incident' ? 'columns.started' : 'columns.scheduled_for'),
                width: 180,
                cell: (event) => (
                  <span className="tabular-nums text-tx-3">{formatMicrosActive(event.started_at)}</span>
                ),
              },
              {
                key: 'updated',
                header: t('columns.updated'),
                width: 180,
                cell: (event) => (
                  <span className="tabular-nums text-tx-3">{formatMicrosActive(event.updated_at)}</span>
                ),
              },
            ]}
          />
        </div>
      )}

      <StatusPageEventDrawer kind={kind} eventId={eventId} onClose={closeDrawer} />
    </PageBody>
  );
}
