import { Flame } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useSearchParams } from 'react-router-dom';

import type { HeatmapParams } from '@/api/rum';
import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { ElementList } from './ElementList';
import { type Device, DEVICES, deviceFilters, type MapKind, parseDevice, parseKind, reachedAt } from './model';
import { HeatmapStage } from './Stage';
import { useClickMap, useScrollMap, useSnapshot } from './useHeatmap';
import { formatCount } from '../../overview/format';
import { isBrowser, scopeFilters } from '../../overview/model';
import { CardState, OverviewCard } from '../../overview/OverviewCard';
import { CONTROL } from '../../overview/ScopeRow';
import type { ScopeContext } from '../../ScopedFrame';
import { sessionPath } from '../../session/moment';
import { useSuggestions } from '../useAnalytics';
import { ValueCombobox } from '../ValueCombobox';

const PAGE_FIELD = 'view.url_path_group';

/**
 * Where people click on a page and how far down they go, drawn over a recording of the page. A map is of
 * one page in one size of window, because a page is laid out differently in each. Without a recording the
 * numbers still stand: the elements in a list, the depths in a table. Everything asked is in the address.
 */
export function HeatmapView({ context }: { context: ScopeContext }) {
  const { t } = useTranslation('rum');
  const { application, scope, range, language, window, setWindow } = context;
  const [params, setParams] = useSearchParams();
  const browser = isBrowser(application.platform);

  const kind = parseKind(params.get('map'));
  const device = parseDevice(params.get('device'));
  const shot = params.get('shot');

  const change = React.useCallback(
    (changes: Record<string, string | null>) =>
      setParams(
        (current) => {
          const next = new URLSearchParams(current);

          for (const [name, value] of Object.entries(changes)) {
            if (value) next.set(name, value);
            else next.delete(name);
          }

          return next;
        },
        { replace: true },
      ),
    [setParams],
  );

  const suggestions = useSuggestions('views', PAGE_FIELD, scope, range);
  const pages = (suggestions.data?.values ?? []).map((entry) => entry.value).filter((value) => value !== '');
  const page = params.get('page') ?? pages[0] ?? '';

  const request = React.useMemo<HeatmapParams | null>(
    () =>
      browser && page
        ? {
            from: range.from,
            to: range.to,
            filters: scopeFilters(scope, [{ field: PAGE_FIELD, op: '=', value: page }, ...deviceFilters(device)]),
          }
        : null,
    [browser, device, page, range, scope],
  );

  const clicks = useClickMap(kind === 'clicks' ? request : null);
  const scroll = useScrollMap(kind === 'scroll' ? request : null);
  const snapshot = useSnapshot(request, shot);
  const [off, setOff] = React.useState<ReadonlySet<string>>(new Set());
  const onLocated = React.useCallback((located: ReadonlySet<string>) => setOff(located), []);

  if (!browser) {
    return <ProductState variant="empty" title={t('analytics.heatmap.not_browser.title')} description={t('analytics.heatmap.not_browser.description')} />;
  }

  const drawn = snapshot.data?.snapshot ?? null;
  const candidates = snapshot.data?.candidates ?? [];
  const read = kind === 'clicks' ? clicks : scroll;
  const empty = kind === 'clicks' ? (clicks.data?.elements.length ?? 0) === 0 : (scroll.data?.views ?? 0) === 0;

  return (
    <div data-rum-analytics="heatmap" className="space-y-[12px]">
      <OverviewCard
        icon={Flame}
        title={t('analytics.heatmap.title')}
        description={t(`analytics.heatmap.description_${kind}`)}
        testId="heatmap-controls"
        bodyClassName="flex flex-wrap items-center gap-2 px-3 py-2"
      >
        <ValueCombobox
          value={page}
          onChange={(next) => change({ page: next || null, shot: null })}
          suggestions={pages}
          placeholder={t('analytics.heatmap.page_placeholder')}
          label={t('analytics.heatmap.page')}
          className={cn(CONTROL, 'min-w-48')}
        />
        <ViewTabs<MapKind>
          label={t('analytics.heatmap.kind')}
          value={kind}
          onValueChange={(next) => change({ map: next === 'clicks' ? null : next })}
          dataAttributes={{ 'data-rum-heatmap-kind': '' }}
          items={[
            { key: 'clicks', label: t('analytics.heatmap.kinds.clicks') },
            { key: 'scroll', label: t('analytics.heatmap.kinds.scroll') },
          ]}
        />
        <FittedSelect
          aria-label={t('analytics.heatmap.device')}
          data-rum-heatmap-device=""
          value={device}
          onValueChange={(next) => change({ device: next === 'desktop' ? null : next, shot: null })}
          options={DEVICES.map((name: Device) => ({ value: name, label: t(`analytics.heatmap.devices.${name}`) }))}
          maxWidth={200}
          prefix={t('analytics.heatmap.device')}
          className={cn(CONTROL, 'font-strong text-tx-1')}
        />
        {candidates.length > 1 && (
          <FittedSelect
            aria-label={t('analytics.heatmap.recording')}
            data-rum-heatmap-recording=""
            value={drawn?.viewId ?? candidates[0]?.viewId ?? ''}
            onValueChange={(next) => change({ shot: next })}
            options={candidates.map((candidate, index) => ({
              value: candidate.viewId,
              label: t('analytics.heatmap.recording_number', { number: index + 1 }),
            }))}
            maxWidth={200}
            prefix={t('analytics.heatmap.recording')}
            className={cn(CONTROL, 'font-strong text-tx-1')}
          />
        )}
        {drawn && (
          <Link
            data-rum-heatmap-session=""
            to={sessionPath(drawn.sessionId, { at: drawn.atTime })}
            className="ml-auto font-sans text-xs text-[var(--blue-ink)] outline-none underline-offset-4 hover:underline focus-visible:underline"
          >
            {t('analytics.heatmap.open_session')}
          </Link>
        )}
      </OverviewCard>

      {request === null && suggestions.isSuccess ? (
        <ProductState
          variant="empty"
          title={t('analytics.heatmap.no_pages.title')}
          description={t('analytics.heatmap.no_pages.description')}
          action={<ChromeButton onClick={() => setWindow(widenTimeWindow(window))}>{t('overview.empty.widen')}</ChromeButton>}
        />
      ) : (
        <div className="grid min-w-0 items-start gap-[12px] xl:grid-cols-[minmax(0,1fr)_320px]">
          <OverviewCard title={page || t('analytics.heatmap.title')} testId="heatmap-stage" className="min-w-0">
            {drawn ? (
              <HeatmapStage snapshot={drawn} kind={kind} clicks={clicks.data} scroll={scroll.data} onLocated={onLocated} />
            ) : (
              <div data-rum-heatmap-no-recording="" className="grid min-h-60 place-items-center p-6 text-center font-sans text-xs text-tx-3">
                {snapshot.isPending ? t('analytics.heatmap.reading_recording') : t(snapshot.isError ? 'analytics.heatmap.recording_denied' : 'analytics.heatmap.no_recording')}
              </div>
            )}
          </OverviewCard>

          <OverviewCard
            title={t(kind === 'clicks' ? 'analytics.heatmap.elements' : 'analytics.heatmap.depths')}
            description={
              kind === 'clicks'
                ? t('analytics.heatmap.clicks_total', { count: clicks.data?.clicks ?? 0 })
                : t('analytics.heatmap.views_total', { count: scroll.data?.views ?? 0 })
            }
            testId="heatmap-numbers"
            className="min-w-0"
          >
            <CardState query={read} empty={empty} emptyTitle={t(kind === 'clicks' ? 'analytics.heatmap.no_clicks' : 'analytics.heatmap.no_scroll')}>
              {kind === 'clicks' ? (
                <ElementList elements={clicks.data?.elements ?? []} off={off} language={language} />
              ) : (
                <dl className="m-0 grid grid-cols-[minmax(0,1fr)_auto] gap-x-3 gap-y-1 px-3 py-2 font-sans text-xs">
                  {scroll.data?.medianDepth != null && (
                    <>
                      <dt className="text-tx-3">{t('analytics.heatmap.median_depth')}</dt>
                      <dd data-rum-median-depth="" className="m-0 tabular-nums text-tx-0">
                        {formatCount(scroll.data.medianDepth, language)} px
                      </dd>
                    </>
                  )}
                  {scroll.data?.pageHeight != null && (
                    <>
                      <dt className="text-tx-3">{t('analytics.heatmap.page_height')}</dt>
                      <dd className="m-0 tabular-nums text-tx-0">{formatCount(Math.round(scroll.data.pageHeight), language)} px</dd>
                    </>
                  )}
                  {(scroll.data?.bands ?? [])
                    .filter((_, index) => index % 5 === 0)
                    .map((band) => (
                      <React.Fragment key={band.depth}>
                        <dt className="tabular-nums text-tx-3">{formatCount(band.depth, language)} px</dt>
                        <dd data-rum-reached={band.depth} className="m-0 tabular-nums text-tx-0">
                          {Math.round(reachedAt(scroll.data?.bands ?? [], band.depth) * 100)}%
                        </dd>
                      </React.Fragment>
                    ))}
                </dl>
              )}
            </CardState>
          </OverviewCard>
        </div>
      )}
    </div>
  );
}
