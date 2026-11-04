import { X } from 'lucide-react';
import * as React from 'react';
import { Trans, useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumPlatform } from '@/api/rum';
import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import {
  type Group,
  groupFilters,
  groupWord,
  kindsOfRow,
  mainPage,
  type Narrowing,
  readSamples,
  readShares,
  type Row,
  samplesQuery,
  sessionPath,
  sharesQuery,
  trendQuery,
} from './model';
import { kindKey } from './Table';
import { RumInvestigateButton } from '../agent/InvestigateButton';
import type { RumFocus } from '../agent/prompt';
import { useApplicationRef } from '../agent/useApplication';
import { chartSeries } from '../explorer/viz/answer';
import type { RumVizBars } from '../explorer/viz/spec';
import { formatCount, formatPercent } from '../overview/format';
import { KIND_TEXT } from '../overview/Frustration';
import { isBrowser, type OverviewRange, type OverviewScope, pageField } from '../overview/model';
import { useDrawerAggregate, useViewRows } from '../performance/drawer/reads';
import { PANEL_CLASS, Section, ShareBar } from '../performance/PanelSection';
import { FIGURE } from '../performance/Reading';
import { formatClock } from '../replay/clock';

/**
 * What the frustrated actions of a group are made of: how many sessions they touched hour by hour,
 * which signs they were, the versions and the pages they happened on, and the newest sessions that met
 * them, each of which opens at the moment before the sign when it was recorded.
 */
export function FrustrationPanel({
  row,
  group,
  platform,
  scope,
  range,
  bars,
  narrowing,
  language,
  onClose,
}: {
  row: Row;
  group: Group;
  platform: RumPlatform;
  scope: OverviewScope;
  range: OverviewRange;
  bars: RumVizBars;
  narrowing: Narrowing;
  language: string;
  onClose: () => void;
}) {
  const { t } = useTranslation('rum');
  const formatter = useTimeFormatter();
  const browser = isBrowser(platform);
  const filters = React.useMemo(() => groupFilters(group, row.values), [group, row.values]);
  const page = pageField(platform);

  const trend = useDrawerAggregate('frustration-trend', trendQuery(scope, bars, narrowing, filters));
  const versionsRead = useDrawerAggregate('frustration-versions', group.key === 'version' ? null : sharesQuery(scope, range, narrowing, filters, 'version'));
  const pagesRead = useDrawerAggregate('frustration-pages', group.key === 'page' ? null : sharesQuery(scope, range, narrowing, filters, page));
  const samplesRead = useViewRows(samplesQuery(scope, range, narrowing, filters));

  const series = React.useMemo(
    () =>
      trend.data
        ? chartSeries(trend.data, {
            bars,
            measures: [{ fn: 'count_distinct', field: 'session.id' }],
            groups: [],
            language,
            none: '',
            measureLabel: () => t('frustration.panel.trend_series'),
          })
        : [],
    [bars, language, t, trend.data],
  );

  const versions = React.useMemo(() => readShares(versionsRead.data), [versionsRead.data]);
  const pages = React.useMemo(() => readShares(pagesRead.data), [pagesRead.data]);
  const samples = React.useMemo(() => readSamples(samplesRead.data ?? []), [samplesRead.data]);
  const kinds = kindsOfRow(row);
  const loudest = kinds[0]?.actions ?? 0;
  const mainly = mainPage(pages, row.sessions);

  const first = row.values[0] ?? null;
  const selector = group.key === 'element' && browser ? (row.values[1] ?? null) : null;
  const title = first ?? t(group.key === 'element' ? 'frustration.table.unnamed' : 'frustration.table.none');
  const application = useApplicationRef(scope.application);

  // An element is asked about as an element; any other group as the views and actions that have its value.
  const focus: RumFocus =
    group.key === 'element' && first !== null
      ? { kind: 'element', name: first, selector }
      : { kind: 'slice', dimension: t(`frustration.groups.${groupWord(group.key, browser)}`), value: title };

  return (
    <aside data-rum-panel="frustration" aria-label={t('frustration.panel.aria')} className={PANEL_CLASS}>
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className={cn('truncate text-sm font-strong', first === null ? 'text-tx-3' : 'text-tx-0', group.key === 'page' && 'font-mono')} title={title}>
            {title}
          </div>
          {selector && (
            <div className="truncate font-mono text-xs text-tx-3" title={selector}>
              {selector}
            </div>
          )}
        </div>
        <button
          type="button"
          aria-label={t('frustration.panel.close')}
          onClick={onClose}
          className="inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md border-0 bg-transparent p-0 text-tx-2 outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
        >
          <X aria-hidden className="h-3.5 w-3.5" />
        </button>
      </div>

      <p data-rum-panel-sentence="" className="m-0 text-sm leading-[21px] text-tx-1">
        <Trans
          i18nKey="frustration.panel.sentence"
          ns="rum"
          count={row.sessions}
          values={{ formatted: formatCount(row.sessions, language), noun: t(`frustration.panel.nouns.${groupWord(group.key, browser)}`) }}
          components={{ n: FIGURE }}
        />
        {mainly && (
          <>
            {' '}
            <Trans i18nKey="frustration.panel.page" ns="rum" values={{ page: mainly }} components={{ code: <code className="font-mono text-tx-0" /> }} />
          </>
        )}
      </p>

      <div>
        <RumInvestigateButton
          look="label"
          label={t(group.key === 'element' ? 'agent.investigate_element' : 'agent.investigate_short')}
          title={t('agent.investigate_title')}
          subject={{ application, environment: scope.environment, range, focus }}
        />
      </div>

      <Section
        title={t('frustration.panel.trend')}
        pending={trend.isPending}
        failed={trend.isError}
        unavailable={trend.data?.unavailable}
        onRetry={() => void trend.refetch()}
        empty={!series.some((line) => line.data.some((value) => typeof value === 'number' && value > 0))}
        emptyText={t('frustration.panel.trend_empty')}
        testId="trend"
      >
        <TimeSeriesChart
          height={120}
          series={series}
          xDomain={[bars.frame.start, bars.frame.end]}
          ariaLabel={t('frustration.panel.trend_aria')}
          cursorScopeId="rum-frustration"
          showLegend={false}
          options={{
            drawStyle: 'bar',
            showPoints: 'never',
            legendMode: 'hidden',
            tooltipMode: 'all',
            leftAxis: { scale: 'linear', min: 0 },
          }}
        />
      </Section>

      <Section
        title={t('frustration.panel.kinds')}
        pending={false}
        failed={false}
        onRetry={() => undefined}
        empty={kinds.length === 0}
        emptyText={t('frustration.panel.kinds_empty')}
        testId="kinds"
      >
        <div data-rum-kinds="" className="flex flex-col gap-1.5">
          {kinds.map((entry) => (
            <div key={entry.kind} data-rum-kind={entry.kind}>
              <ShareBar
                label={t(kindKey(entry.kind, browser))}
                labelWidth="96px"
                ink={KIND_TEXT[entry.kind]}
                share={loudest > 0 ? entry.actions / loudest : 0}
                value={formatCount(entry.actions, language)}
                fill={`var(--${KIND_FILL[entry.kind]})`}
              />
            </div>
          ))}
        </div>
      </Section>

      {group.key !== 'version' && (
        <Section
          title={t('frustration.panel.versions')}
          pending={versionsRead.isPending}
          failed={versionsRead.isError}
          unavailable={versionsRead.data?.unavailable}
          onRetry={() => void versionsRead.refetch()}
          empty={versions.length === 0}
          emptyText={t('frustration.panel.versions_empty')}
          testId="versions"
        >
          <div data-rum-versions="" className="flex flex-col gap-1.5">
            {versions.map((share) => {
              const part = sessionsShare(share.sessions, versions, row.sessions);

              return (
                <div key={share.value ?? ''} data-rum-version={share.value ?? ''}>
                  <ShareBar
                    label={share.value ?? t('frustration.panel.unknown')}
                    mono={share.value !== null}
                    labelWidth="96px"
                    share={part}
                    value={formatPercent(part)}
                    fill="var(--blue)"
                  />
                </div>
              );
            })}
          </div>
        </Section>
      )}

      {group.key !== 'page' && (
        <Section
          title={t(browser ? 'frustration.panel.pages' : 'frustration.panel.screens')}
          pending={pagesRead.isPending}
          failed={pagesRead.isError}
          unavailable={pagesRead.data?.unavailable}
          onRetry={() => void pagesRead.refetch()}
          empty={pages.length === 0}
          emptyText={t('frustration.panel.pages_empty')}
          testId="pages"
        >
          <div data-rum-pages="" className="flex flex-col gap-1.5">
            {pages.map((share) => {
              const part = sessionsShare(share.sessions, pages, row.sessions);

              return (
                <div key={share.value ?? ''} data-rum-page={share.value ?? ''}>
                  <ShareBar
                    label={share.value ?? t('frustration.panel.unknown')}
                    mono={share.value !== null}
                    labelWidth="96px"
                    share={part}
                    value={formatPercent(part)}
                    fill="var(--blue)"
                  />
                </div>
              );
            })}
          </div>
        </Section>
      )}

      <Section
        title={t('frustration.panel.samples')}
        pending={samplesRead.isPending}
        failed={samplesRead.isError}
        onRetry={() => void samplesRead.refetch()}
        empty={samples.length === 0}
        emptyText={t('frustration.panel.samples_empty')}
        testId="samples"
      >
        <ol data-rum-samples="" className="m-0 flex list-none flex-col p-0">
          {samples.map((sample) => (
            <li key={sample.session} data-rum-sample={sample.session} className="border-t border-bd-0 py-2 first:border-t-0 first:pt-0">
              <div className="flex items-start gap-2 text-xs">
                <div className="min-w-0 flex-1">
                  <div className="truncate text-tx-0">
                    {[formatter.micros(sample.at, false), sample.user && (sample.anonymous ? t('frustration.panel.anonymous', { id: sample.user.slice(-4) }) : sample.user)]
                      .filter(Boolean)
                      .join(' · ')}
                  </div>
                  <div className="truncate text-tx-3">
                    {sample.kinds
                      .map((entry) => (entry.count > 1 ? `${t(kindKey(entry.kind, browser))} ×${entry.count}` : t(kindKey(entry.kind, browser))))
                      .join(' · ')}
                    {sample.withError && ` · ${t('frustration.panel.with_error')}`}
                  </div>
                </div>
                <Link
                  to={sessionPath(sample)}
                  data-rum-replay={sample.replay ? '' : undefined}
                  aria-label={t(sample.replay ? 'frustration.panel.replay_aria' : 'frustration.panel.session_aria', { time: formatter.micros(sample.at, false) })}
                  className="inline-flex h-6 shrink-0 items-center rounded-md border-0 bg-[var(--control-surface)] px-2 text-xs font-strong text-tx-1 no-underline outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
                >
                  {sample.replay
                    ? sample.offset === null
                      ? t('frustration.panel.replay')
                      : t('frustration.panel.replay_at', { time: formatClock(sample.offset) })
                    : t('frustration.panel.session')}
                </Link>
              </div>
            </li>
          ))}
        </ol>
      </Section>
    </aside>
  );
}

/** The fill of a sign's bar: the colour its word is told in. */
const KIND_FILL = { rage: 'orange', dead: 'yellow', error: 'red' } as const;

/**
 * The part of the sessions of a group a value held. A session can change version or page, so the parts
 * can come to more than the whole: they are taken against the larger of the two.
 */
function sessionsShare(sessions: number, shares: readonly { sessions: number }[], total: number): number {
  const whole = Math.max(total, shares.reduce((sum, share) => sum + share.sessions, 0));

  return whole > 0 ? Math.min(1, sessions / whole) : 0;
}
