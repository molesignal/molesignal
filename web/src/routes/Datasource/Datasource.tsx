import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate, useParams, useSearchParams } from 'react-router-dom';

import * as homeApi from '@/api/home';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import {
  surfacePageRootClass,
  surfacePanelClass,
  SurfacePageHeader,
} from '@/shell/SurfaceWorkbench';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';
import { ViewTabs } from '@/shell/tabs/ViewTabs';

import { isIntakeSignal } from '../streams/datasourceLink';
import { CatalogueRail } from './catalogue/Rail';
import {
  categoryForSource,
  PRIMARY_CATEGORIES,
  primaryCategoryFromRoute,
  searchSources,
  sendsSignal,
  SIGNALS,
  sourcesInCategory,
  type SignalFilter,
} from './datasourceModel';
import { Guide } from './guide/Guide';
import { RECEIPT_WINDOW_SECS } from './guide/receipt';
import { SOURCES, type Source } from './sources';
import { useSourceText } from './sourceText';

const DEFAULT_STREAM = 'default';

/**
 * The catalogue of sources and, beside it, the guide of the one in view.
 *
 * The address holds what is being looked at: the category and source in its
 * path, the signal (and the stream a link came for) in its query. So a guide
 * can be linked to, reloaded and stepped back from.
 */
export function Datasource() {
  const { t } = useTranslation('onboarding');
  const text = useSourceText();
  const params = useParams();
  const location = useLocation();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const [query, setQuery] = React.useState('');

  const signalParam = searchParams.get('signal');
  const signal: SignalFilter = isIntakeSignal(signalParam) ? signalParam : 'all';
  const streamName = searchParams.get('stream')?.trim() || DEFAULT_STREAM;

  // A source named by the address is shown under a category that holds it, whatever category
  // the address says. One that does not send the signal in view gives way to the first that does.
  const routed = params.source
    ? SOURCES.find((source) => source.id === params.source)
    : undefined;
  const requestedCategory = primaryCategoryFromRoute(params.category);
  const category =
    routed && sendsSignal(routed, signal)
      ? categoryForSource(routed, requestedCategory)
      : requestedCategory;
  const catalogue = React.useMemo(
    () => sourcesInCategory(SOURCES, category, signal),
    [category, signal],
  );
  const selected = routed && catalogue.includes(routed) ? routed : catalogue[0];

  const path = selected ? `/datasource/${category}/${selected.id}` : `/datasource/${category}`;
  React.useEffect(() => {
    if (location.pathname !== path) {
      navigate({ pathname: path, search: location.search }, { replace: true });
    }
  }, [location.pathname, location.search, navigate, path]);

  // Leaving for another category ends the search that was running in this one.
  React.useEffect(() => setQuery(''), [category]);

  // Another source's guide starts at its top, wherever the last one was left.
  const guideRef = React.useRef<HTMLElement>(null);
  const selectedId = selected?.id;
  React.useLayoutEffect(() => {
    if (guideRef.current) guideRef.current.scrollTop = 0;
  }, [selectedId]);

  const searching = query.trim() !== '';
  const results = React.useMemo(
    () => searchSources(SOURCES, query, (source) => [text.name(source), text.description(source)]),
    [query, text],
  );

  const overviewQuery = useQuery({
    queryKey: ['home-overview', 'datasource', RECEIPT_WINDOW_SECS],
    queryFn: () => homeApi.overview({ windowSecs: RECEIPT_WINDOW_SECS, bucketCount: 6 }),
    staleTime: 15_000,
    refetchInterval: 30_000,
  });
  const refreshOverview = React.useCallback(async (): Promise<homeApi.HomeOverview> => {
    const result = await overviewQuery.refetch();
    if (result.error) throw result.error;
    if (!result.data) throw new Error(t('datasource_page.validation_overview_unavailable'));
    return result.data;
  }, [overviewQuery, t]);

  const setSignal = (next: SignalFilter) => {
    const nextParams = new URLSearchParams(searchParams);
    if (next === 'all') nextParams.delete('signal');
    else nextParams.set('signal', next);
    // A stream named by a link belongs to the signal it came with.
    nextParams.delete('stream');
    setSearchParams(nextParams, { replace: true });
  };

  // A source found by search may not send the signal in view; the signal gives way to it.
  const linkFor = (source: Source) => {
    const kept = new URLSearchParams(searchParams);
    if (!sendsSignal(source, signal)) {
      kept.delete('signal');
      kept.delete('stream');
    }
    const search = kept.toString();
    return {
      pathname: `/datasource/${categoryForSource(source, category)}/${source.id}`,
      search: search ? `?${search}` : '',
    };
  };

  const categoryLabel = t(`datasource_page.categories.${category}`);
  const nothingForSignal = {
    message: t('datasource_page.no_signal_match', {
      category: categoryLabel,
      signal: t(`datasource_page.signals.${signal}`),
    }),
    action: t('datasource_page.show_all_signals'),
    onAction: () => setSignal('all'),
  };

  return (
    <section
      data-page-appearance="surface"
      className={cn(
        'flex h-[calc(100vh-var(--topbar-h)-var(--contextbar-h,0px))] min-h-0 flex-col overflow-hidden',
        surfacePageRootClass,
      )}
    >
      <SurfacePageHeader
        title={t('datasource_page.title')}
        subtitle={t('datasource_page.subtitle')}
        breadcrumbs={null}
        backTo={null}
      />

      <ModuleTabs
        label={t('datasource_page.category_navigation')}
        className="shrink-0"
        dataAttributes={{ 'data-datasource-navigation': 'surface' }}
        items={PRIMARY_CATEGORIES.map((item) => {
          const first = sourcesInCategory(SOURCES, item, signal)[0];
          return {
            key: item,
            label: t(`datasource_page.categories.${item}`),
            to: {
              pathname: first ? `/datasource/${item}/${first.id}` : `/datasource/${item}`,
              search: location.search,
            },
            active: item === category,
          };
        })}
        end={
          <ViewTabs<SignalFilter>
            label={t('datasource_page.signal_filter')}
            items={[
              { key: 'all', label: t('datasource_page.signals.all') },
              ...SIGNALS.map((item) => ({
                key: item,
                label: t(`datasource_page.signals.${item}`),
              })),
            ]}
            value={signal}
            onValueChange={setSignal}
          />
        }
      />

      <div className="flex min-h-0 flex-1 gap-[12px] overflow-hidden px-[20px] pb-[20px]">
        <CatalogueRail
          sources={searching ? results : catalogue}
          selectedId={selectedId}
          query={query}
          onQueryChange={setQuery}
          linkFor={linkFor}
          onPick={() => setQuery('')}
          onSubmit={() => {
            const first = results[0];
            if (!first) return;
            navigate(linkFor(first));
            setQuery('');
          }}
          categoryOf={(source) =>
            t(`datasource_page.categories.${categoryForSource(source, category)}`)
          }
          empty={
            searching
              ? {
                  message: t('datasource_page.no_match', { search: query.trim() }),
                  action: { label: t('datasource_page.clear_search'), run: () => setQuery('') },
                }
              : // The guide's side says why and offers the way out.
                { message: t('datasource_page.no_sources') }
          }
          text={text}
        />

        <section
          ref={guideRef}
          data-datasource-guide-surface
          aria-label={t('datasource_page.guide_region')}
          className={cn('min-h-0 min-w-0 flex-1 overflow-auto', surfacePanelClass)}
        >
          {selected ? (
            <Guide
              // What was typed and checked belongs to the source, not to the signal in view.
              key={selected.id}
              source={selected}
              signal={signal === 'all' ? undefined : signal}
              streamName={streamName}
              overview={overviewQuery.data}
              refreshOverview={refreshOverview}
            />
          ) : (
            <div className="grid h-full place-items-center px-6 text-center">
              <div>
                <p className="m-0 font-sans text-md text-tx-2">{nothingForSignal.message}</p>
                <ChromeButton className="mt-4" onClick={nothingForSignal.onAction}>
                  {nothingForSignal.action}
                </ChromeButton>
              </div>
            </div>
          )}
        </section>
      </div>
    </section>
  );
}
