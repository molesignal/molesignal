import {
  Boxes,
  Columns3,
  GitBranch,
  Network,
  Rows3,
  Search,
  Tags,
  X,
  type LucideIcon,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { TopologyResponse } from '@/api/web';
import { cn } from '@/shell/lib/cn';
import { QueryState, queryStateFor } from '@/shell/query/State';
import type { TopologyDirection } from '@/viz/topology/forceLayout';
import {
  ServiceTopology,
  type TopologyLayoutMode,
} from '@/viz/topology/ServiceTopology';

interface ServiceGraphPanelProps {
  range: { from: string; to: string };
  data: TopologyResponse | undefined;
  isLoading: boolean;
  isError: boolean;
  error: unknown;
  onServiceSelect: (serviceId: string) => void;
}

export function ServiceGraphPanel({
  range,
  data,
  isLoading,
  isError,
  error,
  onServiceSelect,
}: ServiceGraphPanelProps) {
  const { t } = useTranslation('traces');
  const [layout, setLayout] = React.useState<TopologyLayoutMode>('tree');
  const [direction, setDirection] = React.useState<TopologyDirection>('horizontal');
  const [search, setSearch] = React.useState('');
  const [showTypes, setShowTypes] = React.useState(true);
  const normalizedSearch = search.trim().toLocaleLowerCase();
  const services = data?.nodes ?? [];
  const matchCount = normalizedSearch
    ? services.filter((service) => service.name.toLocaleLowerCase().includes(normalizedSearch)).length
    : services.length;
  const state = queryStateFor({ isLoading, isError, data: services });

  return (
    <section
      aria-label={t('explore.service_graph.title')}
      className="flex h-full min-h-0 flex-col overflow-hidden bg-bg-0"
    >
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-bd-0 bg-bg-1 px-3 py-2">
        <label className="flex h-9 min-w-[220px] flex-1 items-center gap-2 rounded-md border border-bd-1 bg-bg-0 px-3 text-tx-2 transition-colors focus-within:bg-bg-1 sm:max-w-[340px]">
          <Search className="h-3.5 w-3.5 shrink-0" aria-hidden="true" />
          <input
            value={search}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t('explore.service_graph.search_placeholder')}
            aria-label={t('explore.service_graph.search_aria')}
            className="min-w-0 flex-1 bg-transparent font-sans text-sm text-tx-0 outline-none placeholder:text-tx-3"
          />
          {search && (
            <button
              type="button"
              onClick={() => setSearch('')}
              aria-label={t('explore.service_graph.clear_search')}
              className="grid h-6 w-6 shrink-0 place-items-center rounded text-tx-3 hover:bg-bg-3 hover:text-tx-0"
            >
              <X className="h-3.5 w-3.5" aria-hidden="true" />
            </button>
          )}
        </label>

        <SegmentedControl
          ariaLabel={t('explore.service_graph.layout_aria')}
          value={layout}
          onChange={setLayout}
          options={[
            { value: 'tree', label: t('explore.service_graph.tree_view'), icon: GitBranch },
            { value: 'graph', label: t('explore.service_graph.graph_view'), icon: Network },
          ]}
        />

        <SegmentedControl
          ariaLabel={t('explore.service_graph.direction_aria')}
          value={direction}
          onChange={setDirection}
          options={[
            { value: 'horizontal', label: t('explore.service_graph.horizontal'), icon: Columns3 },
            { value: 'vertical', label: t('explore.service_graph.vertical'), icon: Rows3 },
          ]}
        />

        <div className="hidden h-6 w-px bg-bd-0 xl:block" aria-hidden="true" />
        <HealthLegend />

        <div className="ml-auto flex h-9 shrink-0 items-center gap-2 border-l border-bd-0 pl-3 font-sans text-xs text-tx-2">
          <span className="inline-flex items-center gap-1.5 whitespace-nowrap">
            <Boxes className="h-3.5 w-3.5" aria-hidden="true" />
            {normalizedSearch
              ? t('explore.service_graph.match_count', { visible: matchCount, total: services.length })
              : t('explore.service_graph.entity_count', { count: services.length })}
          </span>
          <button
            type="button"
            aria-pressed={showTypes}
            aria-label={showTypes
              ? t('explore.service_graph.hide_types')
              : t('explore.service_graph.show_types')}
            onClick={() => setShowTypes((current) => !current)}
            className={cn(
              'inline-flex h-8 items-center gap-1.5 rounded-md px-2.5 font-sans text-xs font-semibold transition-colors hover:bg-bg-3 hover:text-tx-0',
              showTypes ? 'bg-indigo-dim text-indigo-soft' : 'text-tx-2',
            )}
          >
            <Tags className="h-3.5 w-3.5" aria-hidden="true" />
            <span className="hidden 2xl:inline">{t('explore.service_graph.show_types')}</span>
          </button>
        </div>
      </div>

      <div
        data-testid="service-graph-canvas"
        className="relative min-h-[320px] flex-1 overflow-hidden bg-bg-0"
      >
        {state ? (
          <QueryState
            state={state}
            error={error}
            loadingLabel={t('explore.service_graph.loading')}
            emptyLabel={t('explore.service_graph.empty')}
            className="h-full"
          />
        ) : (
          <ServiceTopology
            from={range.from}
            to={range.to}
            topology={data}
            layout={layout}
            direction={direction}
            searchQuery={search}
            showServiceTypes={showTypes}
            showEdgeMetrics={layout === 'graph'}
            showBackground={false}
            showMiniMap={false}
            onNodeClick={onServiceSelect}
          />
        )}

        {!state && normalizedSearch && matchCount === 0 && (
          <div className="pointer-events-none absolute left-1/2 top-4 -translate-x-1/2 rounded-md border border-bd-1 bg-bg-1/95 px-3 py-2 font-sans text-xs text-tx-2 shadow-sm">
            {t('explore.service_graph.no_match', { query: search.trim() })}
          </div>
        )}
      </div>
    </section>
  );
}

function HealthLegend() {
  const { t } = useTranslation('traces');
  const items = [
    { key: 'healthy', color: 'border-green' },
    { key: 'degraded', color: 'border-yellow' },
    { key: 'warning', color: 'border-orange' },
    { key: 'critical', color: 'border-red' },
  ] as const;

  return (
    <div
      aria-label={t('explore.service_graph.health_legend')}
      className="type-micro flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 font-sans text-tx-2"
    >
      <span className="hidden font-semibold text-tx-3 2xl:inline">
        {t('explore.service_graph.border_color')}
      </span>
      {items.map((item) => (
        <span key={item.key} className="inline-flex items-center gap-1 whitespace-nowrap">
          <span aria-hidden="true" className={cn('h-2.5 w-2.5 rounded-full border-2 bg-bg-1', item.color)} />
          {t(`explore.service_graph.health.${item.key}`)}
        </span>
      ))}
    </div>
  );
}

interface SegmentedOption<T extends string> {
  value: T;
  label: string;
  icon: LucideIcon;
}

function SegmentedControl<T extends string>({
  ariaLabel,
  value,
  options,
  onChange,
}: {
  ariaLabel: string;
  value: T;
  options: Array<SegmentedOption<T>>;
  onChange: (value: T) => void;
}) {
  return (
    <div
      role="group"
      aria-label={ariaLabel}
      className="flex h-9 shrink-0 items-center rounded-md border border-bd-0 bg-bg-2 p-0.5"
    >
      {options.map((option) => {
        const Icon = option.icon;
        const active = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            aria-pressed={active}
            onClick={() => onChange(option.value)}
            className={cn(
              'inline-flex h-8 items-center gap-1.5 rounded px-2.5 font-sans text-xs font-semibold transition-colors hover:bg-bg-3 hover:text-tx-0',
              active ? 'bg-indigo-dim text-indigo-soft' : 'text-tx-2',
            )}
          >
            <Icon className="h-3.5 w-3.5" aria-hidden={true} />
            <span className="hidden sm:inline">{option.label}</span>
          </button>
        );
      })}
    </div>
  );
}
