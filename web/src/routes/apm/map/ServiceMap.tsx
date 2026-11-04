import { useQuery } from '@tanstack/react-query';
import { Combine, Search, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useSearchParams } from 'react-router-dom';

import { apmMapApi, apmMapQueryKeys, type MapNode } from '@/api/apmMap';
import { FittedSelect } from '@/shell/ui/fitted-select';

import { CallDrawer } from './CallDrawer';
import { ChosenCard } from './ChosenCard';
import { nodeFigures } from './figures';
import { criticalPath, foldNamespaces, FOLD_FROM_NODES, reach } from './graph';
import { type CanvasEdge, type CanvasNode, MapCanvas } from './MapCanvas';
import { nodeHealth, nodeShape } from './MapNodes';
import { ErrorDrawer } from '../errors/ErrorDrawer';
import { FilterToggle } from '../frame/FilterToggle';
import { ApmNavigation } from '../frame/Navigation';
import { ApmPage, ApmQueryState } from '../frame/Page';
import { ApmTimeControls } from '../frame/TimeControls';
import { TraceDrawerHost } from '../frame/TraceDrawerHost';
import { useAddressedDrawer, useApmScope } from '../frame/useApmScope';
import { apmQueryParams, servicePath } from '../scope';
import { useApmSettings } from '../settings/useApmSettings';

const ALL = '__all__';
const CONTROL = 'h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 hover:bg-bg-3';

/**
 * Every service of the environment in view and the calls between them, the
 * databases, caches and outside endpoints they call and the topics they
 * publish to and consume from, as Datadog's service map: a node in the
 * colour of how it fares, a call as thick as its traffic. A node chosen
 * lights what calls it and what it calls, and the slowest way down; a call
 * opens in a drawer. Calls new against the stretch before, or gone since,
 * say so. A big map starts with each namespace folded into one node.
 */
export function ApmServiceMap() {
  const { t } = useTranslation('apm');
  const navigate = useNavigate();
  const { scope, setScope, range, orgId, searchParams } = useApmScope();
  const [, setSearchParams] = useSearchParams();
  const call = useAddressedDrawer('call');
  const namespace = searchParams.get('namespace') ?? '';
  const params = apmQueryParams(scope, range, namespace ? { namespace } : {});
  // A link can open the map with a node chosen, as an incident's blast radius does.
  const [chosen, setChosen] = React.useState<string | null>(() => searchParams.get('node'));
  const [search, setSearch] = React.useState('');
  const [unfolded, setUnfolded] = React.useState<ReadonlySet<string>>(new Set());
  const [folding, setFolding] = React.useState<boolean | null>(null);

  const map = useQuery({
    queryKey: apmMapQueryKeys.map(orgId, params),
    queryFn: () => apmMapApi.map(params),
    enabled: Boolean(orgId),
    refetchInterval: 60_000,
  });

  const data = map.data;
  const seconds = Math.max(1, (range.to.getTime() - range.from.getTime()) / 1_000);
  // A big map folds its namespaces until asked otherwise.
  const fold = folding ?? (data?.nodes.length ?? 0) > FOLD_FROM_NODES;
  const folded = React.useMemo(
    () => new Set(fold ? (data?.namespaces ?? []).filter((value) => !unfolded.has(value)) : []),
    [data?.namespaces, fold, unfolded],
  );
  const drawn = React.useMemo(() => foldNamespaces(data?.nodes ?? [], data?.edges ?? [], folded), [data?.edges, data?.nodes, folded]);

  const figures = React.useCallback((red: MapNode['red']) => nodeFigures(red, seconds), [seconds]);
  const lines = useApmSettings().health;

  const { nodes, edges } = React.useMemo(() => {
    const needle = search.trim().toLocaleLowerCase();
    const lit = chosen ? new Set([chosen, ...reach(drawn.edges, chosen, 'up'), ...reach(drawn.edges, chosen, 'down')]) : null;
    const critical = new Set(chosen ? criticalPath(drawn.edges, chosen) : []);

    const canvasNodes = drawn.nodes.map<CanvasNode>((entry) => {
      const id = entry.node?.id ?? entry.group!.id;
      const name = entry.node?.name ?? entry.group!.namespace;
      const matches = !needle || name.toLocaleLowerCase().includes(needle);
      return {
        id,
        data: entry.node
          ? {
              name,
              shape: nodeShape(entry.node),
              health: nodeHealth(entry.node, lines),
              figures: entry.node.change === 'gone' ? t('map.gone_figures') : figures(entry.node.red),
              change: entry.node.change,
              dimmed: lit ? !lit.has(id) : !matches,
              chosen: id === chosen,
            }
          : {
              name,
              shape: 'group',
              health: entry.group!.health,
              figures: t('map.group_figures', { count: entry.group!.services }),
              dimmed: lit ? !lit.has(id) : !matches,
              chosen: id === chosen,
            },
      };
    });

    const canvasEdges = drawn.edges.map<CanvasEdge>((edge) => ({
      ...edge,
      dimmed: lit ? !(lit.has(edge.source) && lit.has(edge.target)) : false,
      critical: critical.has(edge.id),
    }));

    return { nodes: canvasNodes, edges: canvasEdges };
  }, [chosen, drawn, figures, lines, search, t]);

  const namespaces = data?.namespaces ?? [];
  const environments = data?.environments ?? [];
  const chosenNode = chosen ? data?.nodes.find((node) => node.id === chosen) : undefined;
  const chosenGroup = chosen ? drawn.nodes.find((entry) => entry.group?.id === chosen)?.group : undefined;

  const openService = (node: MapNode) =>
    navigate(servicePath({ namespace: node.namespace ?? '', name: node.name, environment: scope.environment || node.environments?.[0] || '' }, 'overview', searchParams));

  const setNamespace = (value: string) =>
    setSearchParams((current) => {
      const next = new URLSearchParams(current);
      if (value) next.set('namespace', value);
      else next.delete('namespace');
      return next;
    });

  return (
    <TraceDrawerHost>
      <ApmPage
        page="map"
        title={t('title')}
        subtitle={t('subtitle')}
        breadcrumbs={null}
        navigation={
          <ApmNavigation end={<ApmTimeControls scope={scope} onScopeChange={setScope} />}>
            <FittedSelect
              aria-label={t('catalog.environment')}
              value={scope.environment || ALL}
              onValueChange={(value) => setScope({ environment: value === ALL ? '' : value })}
              options={[{ value: ALL, label: t('catalog.all_environments') }, ...environments.map((value) => ({ value, label: value }))]}
              maxWidth={200}
              prefix={scope.environment ? t('catalog.environment') : undefined}
              className={CONTROL}
            />
            <FittedSelect
              aria-label={t('map.namespace')}
              data-apm-map-namespace=""
              value={namespace || ALL}
              onValueChange={(value) => setNamespace(value === ALL ? '' : value)}
              options={[{ value: ALL, label: t('map.all_namespaces') }, ...namespaces.map((value) => ({ value, label: value }))]}
              maxWidth={200}
              prefix={namespace ? t('map.namespace') : undefined}
              className={CONTROL}
            />
          </ApmNavigation>
        }
      >
        <div data-apm-service-map="" className="relative flex h-[calc(100vh-220px)] min-h-[460px] flex-col overflow-hidden rounded-md bg-bg-0 [box-shadow:var(--shadow-functional-surface)]">
          <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-bd-0 bg-bg-1 px-3 py-2 font-sans text-xs">
            <label className="relative">
              <span className="sr-only">{t('map.search')}</span>
              <Search aria-hidden className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-tx-3" />
              <input
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder={t('map.search')}
                className="h-8 w-56 rounded-md border-0 bg-[var(--control-surface)] pl-8 pr-7 text-xs text-tx-1 outline-none placeholder:text-tx-3 hover:bg-bg-3 focus-visible:bg-bg-3"
              />
              {search && (
                <button type="button" aria-label={t('map.clear_search')} onClick={() => setSearch('')} className="absolute right-1.5 top-1.5 grid h-5 w-5 place-items-center rounded text-tx-3 hover:text-tx-0">
                  <X aria-hidden className="h-3.5 w-3.5" />
                </button>
              )}
            </label>
            {namespaces.length > 0 && (
              <FilterToggle
                pressed={fold}
                onToggle={() => {
                  setFolding(!fold);
                  setUnfolded(new Set());
                }}
                icon={<Combine aria-hidden className="h-3.5 w-3.5 text-[var(--blue-ink)]" />}
                data-apm-map-fold=""
              >
                {t('map.fold')}
              </FilterToggle>
            )}
            <span className="ml-auto flex items-center gap-3 text-tx-2" data-apm-map-legend="">
              <span className="font-strong text-[var(--green-ink)]">{t('map.change.new')}</span>
              <span className="font-strong text-tx-3 line-through">{t('map.change.gone')}</span>
              <span className="font-strong text-[var(--accent)]">{t('map.critical')}</span>
            </span>
          </div>
          <div className="relative min-h-0 flex-1">
            <ApmQueryState query={map} empty={!data || data.nodes.length === 0} emptyTitle={t('map.none')} emptyDescription={t('map.none_description')}>
              <MapCanvas
                nodes={nodes}
                edges={edges}
                onNode={(id) => setChosen((current) => (current === id ? null : id))}
                onEdge={(edge) => {
                  // A call between folded namespaces opens when there is one call in it.
                  if (edge.merged.length === 1) call.open(edge.merged[0]!);
                  else setChosen(edge.source);
                }}
                onPane={() => setChosen(null)}
              />
              {data?.truncated && <p className="pointer-events-none absolute left-3 top-3 font-sans text-xs text-tx-2">{t('map.truncated')}</p>}
              {(chosenNode || chosenGroup) && (
                <ChosenCard
                  node={chosenNode}
                  group={chosenGroup}
                  figures={chosenNode ? figures(chosenNode.red) : chosenGroup ? figures(chosenGroup.red) : ''}
                  callers={chosen ? reach(drawn.edges, chosen, 'up').size : 0}
                  callees={chosen ? reach(drawn.edges, chosen, 'down').size : 0}
                  calls={drawn.edges.filter((edge) => edge.source === chosen || edge.target === chosen)}
                  nameOf={(id) => drawn.nodes.find((entry) => (entry.node?.id ?? entry.group?.id) === id)?.node?.name ?? id.replace(/^group:/, '')}
                  onOpenCall={(edge) => {
                    if (edge.merged.length === 1) call.open(edge.merged[0]!);
                  }}
                  onOpenService={openService}
                  onUnfold={(value) => {
                    setUnfolded((current) => new Set([...current, value]));
                    setChosen(null);
                  }}
                  onClose={() => setChosen(null)}
                />
              )}
            </ApmQueryState>
          </div>
        </div>
        <CallDrawer map={data} params={params} orgId={orgId} />
        <ErrorDrawer />
      </ApmPage>
    </TraceDrawerHost>
  );
}

