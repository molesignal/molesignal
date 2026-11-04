import { BellPlus, X } from 'lucide-react';
import * as React from 'react';
import { Trans, useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';
import { rumNewRuleHref } from '@/routes/alerts/rule-workbench/rumSource';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Button } from '@/shell/ui/button';
import { Sheet, SheetClose, SheetContent, SheetTitle } from '@/shell/ui/sheet';
import type { RumAlertMetric } from '@/types/alerting';

import { ElementsCard } from './Elements';
import { MakeupCard } from './Makeup';
import { useDrawerAggregate, useViewRows } from './reads';
import { WorstCard } from './Worst';
import { formatDurationMs } from '../../_helpers';
import type { RumVizBars } from '../../explorer/viz/spec';
import { formatCount, formatPercent } from '../../overview/format';
import { clause, explorerPath, gradeOf, type OverviewRange, type OverviewScope } from '../../overview/model';
import { CardState } from '../../overview/OverviewCard';
import {
  ATTRIBUTIONS,
  type DimensionKey,
  dimensionOf,
  DRAWER_METRICS,
  type DrawerMetric,
  elementsQuery,
  groupFilter,
  groupLevelQuery,
  longest,
  metricOf,
  partsQuery,
  readElements,
  readGroupLevel,
  readMakeup,
  readWorst,
  trendQuery,
  worstQuery,
  worstRowsQuery,
  worstViews,
} from '../model';
import { FIGURE, Reading } from '../Reading';
import { valueLabel } from '../Table';
import { TrendCard } from '../Trend';

const LINK =
  'inline-flex h-8 items-center rounded-md bg-[var(--control-surface)] px-2.5 font-sans text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3';

/** The figure of an alert rule that a vital of the drawer stands for. */
const ALERT_METRIC: Record<DrawerMetric, RumAlertMetric> = { lcp: 'lcp_p75', inp: 'inp_p75', cls: 'cls_p75' };

/**
 * What one value of a dimension is slow on, in a drawer from the right: for the vital in front, how
 * far it is, what it is made of, which elements set it, how it went over the time in view, and the
 * views that were worst. Closing the drawer takes the value out of the address.
 */
export function AttributionDrawer({
  value,
  dimension,
  tab,
  onTab,
  scope,
  range,
  bars,
  language,
  onClose,
  onOpenView,
}: {
  /** The value of the dimension that is open; none is no drawer. */
  value: string;
  dimension: DimensionKey;
  tab: DrawerMetric;
  onTab: (tab: DrawerMetric) => void;
  scope: OverviewScope;
  range: OverviewRange;
  bars: RumVizBars;
  language: string;
  onClose: () => void;
  /** Opens the load of a view over the drawer. */
  onOpenView?: ((view: string, at: number | null) => void) | undefined;
}) {
  const { t } = useTranslation('rum');

  return (
    <Sheet open={value !== ''} onOpenChange={(open) => !open && onClose()}>
      <SheetContent
        side="right"
        hideClose
        overlayClassName="top-topbar bg-overlay-soft"
        aria-describedby={undefined}
        data-rum-attribution-drawer=""
        className={cn('bottom-0 top-topbar flex h-auto max-w-none flex-col gap-0 border-0 bg-[var(--functional-surface)] p-0 shadow-drawer sm:max-w-none')}
        style={{ width: 'min(780px, 100vw - 48px)' }}
      >
        <SheetTitle className="sr-only">{t('performance.drawer.title')}</SheetTitle>
        {value !== '' && (
          <DrawerBody
            key={value}
            value={value}
            dimension={dimension}
            tab={tab}
            onTab={onTab}
            scope={scope}
            range={range}
            bars={bars}
            language={language}
            onOpenView={onOpenView}
          />
        )}
      </SheetContent>
    </Sheet>
  );
}

function DrawerBody({
  value,
  dimension,
  tab,
  onTab,
  scope,
  range,
  bars,
  language,
  onOpenView,
}: {
  value: string;
  dimension: DimensionKey;
  tab: DrawerMetric;
  onTab: (tab: DrawerMetric) => void;
  scope: OverviewScope;
  range: OverviewRange;
  bars: RumVizBars;
  language: string;
  onOpenView?: ((view: string, at: number | null) => void) | undefined;
}) {
  const { t } = useTranslation('rum');
  const metric = metricOf(tab);
  const entry = dimensionOf(dimension);
  const group = React.useMemo(() => groupFilter(entry, value), [entry, value]);
  const name = t(`performance.metrics.${metric.key}`);
  const title = valueLabel(dimension, value, language, t('performance.table.none'));

  const level = useDrawerAggregate('level', groupLevelQuery(scope, range, group, metric));
  const found = readGroupLevel(level.data, metric);
  const parts = useDrawerAggregate('parts', found?.raw != null ? partsQuery(scope, range, group, metric, found.raw) : null);
  const elements = useDrawerAggregate('elements', elementsQuery(scope, range, group, metric));
  const trend = useDrawerAggregate('trend', trendQuery(scope, bars, metric, [group]));
  const worst = useDrawerAggregate('worst', worstQuery(scope, range, group, metric));
  const ranked = React.useMemo(() => readWorst(worst.data, metric), [metric, worst.data]);
  const rows = useViewRows(ranked.length > 0 ? worstRowsQuery(scope, range, ranked.map((candidate) => candidate.id)) : null);

  const makeup = readMakeup(parts.data, metric);
  const top = makeup ? longest(makeup) : null;
  const makeupTotal = makeup?.parts.reduce((sum, part) => sum + part.ms, 0) ?? 0;
  const hasParts = (ATTRIBUTIONS[metric.key]?.parts.length ?? 0) > 0;
  const groupClause = clause(entry.field, value);
  // A rule can be narrowed to a page or to a version, not to a browser, a country or a device.
  const manage = useActionAccess({ permission: 'alerts.rules.manage' });
  const alertable = manage.allowed && (dimension === 'page' || dimension === 'version');

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      <header className="flex shrink-0 flex-wrap items-start justify-between gap-x-4 gap-y-3 border-b border-bd-0 px-5 py-4">
        <div className="min-w-0">
          <h2 data-rum-attribution-title="" className="m-0 truncate type-page-title font-sans font-display text-tx-0">
            {title}
          </h2>
          <p className="m-0 truncate font-sans text-xs text-tx-3">
            {[scope.application, scope.environment, t(`performance.dimensions.${dimension}`)].filter(Boolean).join(' · ')}
            {found && ` · ${t('performance.drawer.views', { count: found.views, formatted: formatCount(found.views, language) })}`}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <ViewTabs
            label={t('performance.drawer.tabs')}
            dataAttributes={{ 'data-rum-drawer-tabs': '' }}
            value={tab}
            onValueChange={onTab}
            items={DRAWER_METRICS.map((key) => ({ key, label: t(`performance.metrics.${key}`) }))}
          />
          {alertable && (
            <Link
              to={rumNewRuleHref({
                application: scope.application,
                environment: scope.environment,
                ...(dimension === 'page' ? { page: value } : { version: value }),
                metric: ALERT_METRIC[tab],
              })}
              className={cn(LINK, 'gap-1.5')}
              data-rum-attribution-alert=""
            >
              <BellPlus aria-hidden className="h-3.5 w-3.5 text-tx-3" />
              {t('performance.drawer.alert')}
            </Link>
          )}
          <Link to={explorerPath('views', scope, [groupClause])} className={LINK} data-rum-attribution-explore="">
            {t('performance.drawer.explore')}
          </Link>
          <SheetClose asChild>
            <Button variant="ghost" size="icon" className="h-8 w-8" aria-label={t('performance.drawer.close')}>
              <X className="h-4 w-4" />
            </Button>
          </SheetClose>
        </div>
      </header>
      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-4">
        <section data-rum-attribution-statement="" className={cn(surfacePanelClass, 'space-y-1 px-4 py-3')}>
          <CardState query={level} empty={found === null || found.value === null} emptyTitle={t('performance.drawer.none', { name })}>
            {found && found.value !== null && (
              <>
                <p className="m-0 font-sans text-sm font-strong text-tx-1">
                  <Trans
                    i18nKey="performance.drawer.level"
                    ns="rum"
                    values={{ name, formatted: formatCount(found.views, language) }}
                    components={{
                      n: FIGURE,
                      reading: <Reading level={{ metric, value: found.value, grade: gradeOf(metric, found.value) }} />,
                    }}
                  />
                </p>
                {top && makeupTotal > 0 && (
                  <p data-rum-attribution-longest="" className="m-0 font-sans text-sm text-tx-1">
                    <Trans
                      i18nKey="performance.drawer.longest"
                      ns="rum"
                      values={{
                        part: t(`performance.parts.${top.key}`),
                        value: formatDurationMs(top.ms),
                        share: formatPercent(top.ms / makeupTotal),
                      }}
                      components={{ n: FIGURE }}
                    />
                  </p>
                )}
              </>
            )}
          </CardState>
        </section>
        {hasParts && <MakeupCard query={parts} makeup={makeup} metric={metric} language={language} />}
        <ElementsCard query={elements} elements={readElements(elements.data, metric)} metric={metric} scope={scope} group={groupClause} language={language} />
        <TrendCard query={trend} bars={bars} metric={metric} language={language} testId="performance-group-trend" height={180} />
        <WorstCard query={worst} worst={worstViews(ranked, rows.data ?? [])} metric={metric} scope={scope} onOpenView={onOpenView} />
      </div>
    </div>
  );
}
