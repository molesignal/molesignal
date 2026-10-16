import { CalendarClock, Plus } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate } from 'react-router-dom';

import type { StatusPageIncident, StatusPageSnapshot } from '@/api/statusPages';
import { formatMicrosActive } from '@/lib/time';
import { ChromeButton, Pill } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { PageBody } from '@/shell/PageHeader';
import { SpectrumBar } from '@/shell/readout/SpectrumBar';
import { formatRelativeSpan } from '@/time/relative';

import {
  INCIDENT_TONE,
  STATUS_TEXT,
  affectedComponentNames,
  componentStatusLabel,
  formatDurationMicros,
  impactLabel,
  incidentStatusLabel,
  namedComponents,
} from '../model';
import {
  STATUS_COLOR,
  type SeenComponent,
  componentSpectrum,
  endedEventPath,
  eventPath,
  eventsUnderWay,
  latestUpdate,
  maintenanceAhead,
  seenComponents,
} from '../summary';
import { useStatusPageWorkspace } from './Layout';
import { StatusPageCanvas, StatusPageSection } from './Surfaces';

/** How many ended events the overview recalls; the History tab has them all. */
const LATELY_COUNT = 4;

const IMPACT_COLOR: Record<StatusPageIncident['impact'], string> = {
  critical: 'var(--red)',
  major: 'var(--orange)',
  minor: 'var(--yellow)',
  maintenance: 'var(--blue)',
};

/**
 * A status page as its readers see it now, for the people who run it.
 *
 * It opens with the verdict the public page gives and how the components
 * divide, as one statement. Under it is what that verdict rests on: the events
 * under way with the last thing said about each, what is planned, and the
 * components that are not well, then the last few things that ended. Nothing
 * is counted twice: the figures are in the statement, the lists are what they
 * count, and a list with nothing in it is not drawn.
 */
export function StatusPageOverview() {
  const { t, i18n } = useTranslation('status-pages');
  const navigate = useNavigate();
  const { pageId, snapshot, manageAccess } = useStatusPageWorkspace();
  const canMutate = manageAccess.allowed && snapshot.page.lifecycle === 'active';
  const components = seenComponents(snapshot);
  const underWay = eventsUnderWay(snapshot);
  const ahead = maintenanceAhead(snapshot);
  const unwell = components.filter((seen) => seen.status !== 'operational');
  const hidden = namedComponents(snapshot.components).filter(
    (component) => component.lifecycle === 'active' && component.visibility === 'hidden',
  ).length;
  const hasEvents = underWay.length > 0 || ahead.length > 0;
  const hasUnwell = unwell.length > 0 || hidden > 0;
  // The last few things that ended: the page's recent record, without leaving the overview.
  const lately = [...snapshot.history]
    .sort((left, right) => (right.ended_at ?? right.updated_at) - (left.ended_at ?? left.updated_at))
    .slice(0, LATELY_COUNT);
  // An event names the components it is about, so the first thing a page needs is a component.
  const needsComponents = !namedComponents(snapshot.components).some((component) => component.lifecycle === 'active');

  return (
    <PageBody className="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2">
      <StatusPageCanvas>
        <section
          data-status-page-standing={snapshot.overall_status}
          className="ms-rise grid gap-x-[48px] gap-y-[24px] rounded-md bg-[var(--functional-surface)] px-[20px] py-[20px] [box-shadow:var(--shadow-functional-surface)] lg:grid-cols-[minmax(0,1fr)_minmax(0,460px)] lg:items-center lg:px-[24px] lg:py-[24px]"
        >
          <div className="min-w-0">
            <p className="type-label font-medium text-tx-2">{t('overview.seen_now')}</p>
            <h2
              className={cn(
                'mt-[6px] flex items-center gap-[10px] font-sans text-2xl font-display tracking-[-0.02em]',
                STATUS_TEXT[snapshot.overall_status],
              )}
            >
              <span
                aria-hidden="true"
                className="h-[10px] w-[10px] shrink-0 rounded-full"
                style={{ background: STATUS_COLOR[snapshot.overall_status] }}
              />
              {componentStatusLabel(t, snapshot.overall_status)}
            </h2>
            <p className="mt-[6px] text-pretty text-sm text-tx-2">
              {standingLine(t, underWay.length, ahead.length)}
              <span className="text-tx-3">
                {' · '}
                {t('overview.last_change', {
                  time: formatRelativeSpan(snapshot.updated_at, snapshot.generated_at, i18n.language),
                })}
              </span>
            </p>
            <div className="mt-[18px] flex flex-wrap items-center gap-2">
              {needsComponents ? (
                <ChromeButton
                  variant="primary"
                  disabled={!canMutate}
                  disabledReason={manageAccess.reason}
                  onClick={() => navigate(`/status-pages/${pageId}/components/new`)}
                >
                  <Plus className="h-3.5 w-3.5" />
                  {t('actions.add_component')}
                </ChromeButton>
              ) : (
                <>
                  <ChromeButton
                    variant="primary"
                    disabled={!canMutate}
                    disabledReason={manageAccess.reason}
                    onClick={() => navigate(`/status-pages/${pageId}/incidents/new`)}
                  >
                    <Plus className="h-3.5 w-3.5" />
                    {t('actions.new_incident')}
                  </ChromeButton>
                  <ChromeButton
                    disabled={!canMutate}
                    disabledReason={manageAccess.reason}
                    onClick={() => navigate(`/status-pages/${pageId}/maintenance/new`)}
                  >
                    <CalendarClock className="h-3.5 w-3.5" />
                    {t('actions.schedule_maintenance')}
                  </ChromeButton>
                </>
              )}
            </div>
          </div>

          <div className="min-w-0">
            <p className="type-label font-medium text-tx-2">
              {t('overview.components_on_page', { count: components.length })}
            </p>
            {components.length === 0 ? (
              <p className="mt-[10px] text-sm text-tx-2">{t('states.no_components_description')}</p>
            ) : (
              <SpectrumBar
                className="mt-[12px]"
                label={t('sections.components')}
                segments={componentSpectrum(t, components)}
              />
            )}
          </div>
        </section>

        {/* What the statement rests on. A part with nothing in it is left out: the statement
            above has already said there is nothing. */}
        {(hasEvents || hasUnwell) && (
          <div
            className={cn(
              'grid grid-cols-1 gap-[12px]',
              hasEvents && hasUnwell && 'xl:grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)]',
            )}
          >
            {hasEvents && (
              <div className="grid min-w-0 content-start gap-[12px]">
                {underWay.length > 0 && (
                  <StatusPageSection title={t('overview.under_way')}>
                    <ul className="divide-y divide-bd-0 border-t border-bd-0">
                      {underWay.map((event) => (
                        <EventRow key={event.id} event={event} pageId={pageId} snapshot={snapshot} />
                      ))}
                    </ul>
                  </StatusPageSection>
                )}
                {ahead.length > 0 && (
                  <StatusPageSection title={t('sections.scheduled_maintenance')}>
                    <ul className="divide-y divide-bd-0 border-t border-bd-0">
                      {ahead.map((event) => (
                        <EventRow key={event.id} event={event} pageId={pageId} snapshot={snapshot} when="planned" />
                      ))}
                    </ul>
                  </StatusPageSection>
                )}
              </div>
            )}

            {hasUnwell && (
              <StatusPageSection title={t('overview.not_operational')} className="self-start">
                {unwell.length > 0 && (
                  <ul className="divide-y divide-bd-0 border-t border-bd-0">
                    {unwell.map((seen) => (
                      <ComponentRow key={seen.component.id} seen={seen} pageId={pageId} />
                    ))}
                  </ul>
                )}
                {hidden > 0 && (
                  <p className="border-t border-bd-0 px-4 py-[10px] text-xs text-tx-2">
                    {t('overview.hidden_components', { count: hidden })}
                  </p>
                )}
              </StatusPageSection>
            )}
          </div>
        )}

        {lately.length > 0 && (
          <StatusPageSection title={t('overview.lately')}>
            <ul className="divide-y divide-bd-0 border-t border-bd-0">
              {lately.map((event) => (
                <EventRow key={event.id} event={event} pageId={pageId} snapshot={snapshot} when="ended" />
              ))}
            </ul>
          </StatusPageSection>
        )}
      </StatusPageCanvas>
    </PageBody>
  );
}

function standingLine(t: ReturnType<typeof useTranslation>['t'], underWay: number, ahead: number): string {
  const parts = [
    underWay > 0 && t('overview.events_under_way', { count: underWay }),
    ahead > 0 && t('overview.maintenance_scheduled', { count: ahead }),
  ].filter(Boolean);

  return parts.length > 0 ? parts.join(' · ') : t('overview.nothing_going_on');
}

/** An event the page tells its readers about, with the last thing it told them. */
function EventRow({
  event,
  pageId,
  snapshot,
  when = 'started',
}: {
  event: StatusPageIncident;
  pageId: string;
  snapshot: StatusPageSnapshot;
  /** Which of its moments the row is about: it began, it is announced for, or it is over. */
  when?: 'started' | 'planned' | 'ended';
}) {
  const { t, i18n } = useTranslation('status-pages');
  const components = affectedComponentNames(event, snapshot.components);
  const latest = latestUpdate(event);
  const endedAt = event.ended_at ?? event.updated_at;
  const distance = (moment: number) => formatRelativeSpan(moment, snapshot.generated_at, i18n.language);
  const moment =
    when === 'planned'
      ? t('overview.planned_for', {
          date: formatMicrosActive(event.started_at, false),
          when: distance(event.started_at),
        })
      : when === 'ended'
        ? `${t('overview.ended', { when: distance(endedAt) })} · ${t('overview.lasted', {
            duration: formatDurationMicros(t, event.started_at, endedAt),
          })}`
        : t('overview.started', { when: distance(event.started_at) });

  return (
    <li>
      <Link
        to={when === 'ended' ? endedEventPath(pageId, event.id) : eventPath(pageId, event)}
        className="block px-4 py-[12px] outline-none transition-colors hover:bg-bg-2 focus-visible:bg-bg-2"
      >
        <div className="flex min-w-0 items-center gap-[8px]">
          <span
            aria-hidden="true"
            className="h-[8px] w-[8px] shrink-0 rounded-full"
            style={{ background: IMPACT_COLOR[event.impact] }}
          />
          <span className="min-w-0 flex-1 truncate font-sans text-sm font-strong text-tx-0">{event.title}</span>
          <Pill tone={INCIDENT_TONE[event.status]}>{incidentStatusLabel(t, event.status, event.kind)}</Pill>
        </div>
        <p className="mt-[4px] truncate pl-[16px] text-xs text-tx-2">
          {[impactLabel(t, event.impact), components.join(', ') || t('values.no_components'), moment].join(' · ')}
        </p>
        {/* What was last said matters while the event is open; once it is over, the row is a record. */}
        {latest && when !== 'ended' && (
          <p className="mt-[6px] line-clamp-2 text-pretty pl-[16px] text-xs leading-relaxed text-tx-1">
            {latest.message}
          </p>
        )}
      </Link>
    </li>
  );
}

function ComponentRow({ seen, pageId }: { seen: SeenComponent; pageId: string }) {
  const { t } = useTranslation('status-pages');

  return (
    <li>
      <Link
        to={`/status-pages/${encodeURIComponent(pageId)}/components/${encodeURIComponent(seen.component.id)}`}
        className="flex min-h-[44px] items-center gap-[10px] px-4 py-[8px] outline-none transition-colors hover:bg-bg-2 focus-visible:bg-bg-2"
      >
        <span
          aria-hidden="true"
          className="h-[8px] w-[8px] shrink-0 rounded-full"
          style={{ background: STATUS_COLOR[seen.status] }}
        />
        <span className="min-w-0 flex-1 truncate font-sans text-sm text-tx-0">{seen.component.name}</span>
        <span className={cn('shrink-0 font-sans text-xs font-strong', STATUS_TEXT[seen.status])}>
          {componentStatusLabel(t, seen.status)}
        </span>
      </Link>
    </li>
  );
}
