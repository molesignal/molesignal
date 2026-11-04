import { Archive, Search, Star } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { FittedSelect } from '@/shell/ui/fitted-select';

import { FilterToggle } from '../frame/FilterToggle';
import type { ApmScope } from '../scope';
import type { CatalogGrouping } from './groups';
import type { CatalogParams } from './params';

const ALL = '__all__';
const NONE = '__none__';
const SELECT = 'h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 hover:bg-bg-3';

export interface CatalogChoices {
  environments: readonly string[];
  teams: ReadonlyArray<{ id: string; name: string }>;
  namespaces: readonly string[];
  /** Every `key=value` the services are tagged with. */
  tags: readonly string[];
  /** Every tag key: what the catalog can be grouped by besides team, namespace and tier. */
  tagKeys: readonly string[];
}

/**
 * What narrows the catalog, in the title of its card: names, environment,
 * team, and the namespace and tag where services have more than one; the
 * reader's favorites and the archived services are each a toggle.
 */
export function CatalogFilters({
  scope,
  onScopeChange,
  params,
  onParam,
  choices,
  favoriteCount,
  archivedCount,
}: {
  scope: ApmScope;
  onScopeChange: (patch: Partial<ApmScope>) => void;
  params: CatalogParams;
  onParam: (key: 'team' | 'namespace' | 'tag' | 'favorites' | 'archived', value: string | boolean) => void;
  choices: CatalogChoices;
  favoriteCount: number;
  archivedCount: number;
}) {
  const { t } = useTranslation('apm');
  const pick = (value: string) => (value === ALL ? '' : value);

  return (
    <span className="flex flex-wrap items-center gap-2">
      <label className="relative">
        <span className="sr-only">{t('catalog.search')}</span>
        <Search aria-hidden className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-tx-3" />
        <input
          value={scope.search}
          onChange={(event) => onScopeChange({ search: event.target.value })}
          placeholder={t('catalog.search')}
          data-apm-catalog-search=""
          className="h-8 w-56 rounded-md border-0 bg-[var(--control-surface)] pl-8 pr-2.5 font-sans text-xs font-body text-tx-1 outline-none placeholder:text-tx-3 hover:bg-bg-3 focus-visible:bg-bg-3"
        />
      </label>
      <FilterToggle
        pressed={params.favorites}
        onToggle={() => onParam('favorites', !params.favorites)}
        icon={<Star aria-hidden className="h-3.5 w-3.5 text-[var(--yellow-ink)]" fill={params.favorites ? 'currentColor' : 'none'} />}
        data-apm-catalog-favorites=""
      >
        {t('catalog.favorites', { count: favoriteCount })}
      </FilterToggle>
      <FittedSelect
        aria-label={t('catalog.environment')}
        value={scope.environment || ALL}
        onValueChange={(value) => onScopeChange({ environment: pick(value) })}
        options={[{ value: ALL, label: t('catalog.all_environments') }, ...choices.environments.map((value) => ({ value, label: value }))]}
        maxWidth={200}
        prefix={scope.environment ? t('catalog.environment') : undefined}
        className={SELECT}
      />
      <FittedSelect
        aria-label={t('catalog.team')}
        value={params.team || ALL}
        onValueChange={(value) => onParam('team', pick(value))}
        options={[{ value: ALL, label: t('catalog.all_teams') }, ...choices.teams.map((team) => ({ value: team.id, label: team.name }))]}
        maxWidth={200}
        prefix={params.team ? t('catalog.team') : undefined}
        className={SELECT}
      />
      {(choices.namespaces.length > 1 || params.namespace) && (
        <FittedSelect
          aria-label={t('catalog.namespace')}
          data-apm-catalog-namespace=""
          value={params.namespace || ALL}
          onValueChange={(value) => onParam('namespace', pick(value))}
          options={[{ value: ALL, label: t('catalog.all_namespaces') }, ...choices.namespaces.map((value) => ({ value, label: value }))]}
          maxWidth={200}
          prefix={params.namespace ? t('catalog.namespace') : undefined}
          className={SELECT}
        />
      )}
      {(choices.tags.length > 0 || params.tag) && (
        <FittedSelect
          aria-label={t('catalog.tag')}
          data-apm-catalog-tag=""
          value={params.tag || ALL}
          onValueChange={(value) => onParam('tag', pick(value))}
          options={[{ value: ALL, label: t('catalog.all_tags') }, ...choices.tags.map((value) => ({ value, label: value }))]}
          maxWidth={240}
          prefix={params.tag ? t('catalog.tag') : undefined}
          className={SELECT}
        />
      )}
      {(archivedCount > 0 || params.archived) && (
        <FilterToggle
          pressed={params.archived}
          onToggle={() => onParam('archived', !params.archived)}
          icon={<Archive aria-hidden className="h-3.5 w-3.5 text-[var(--purple-ink)]" />}
          data-apm-catalog-archived=""
        >
          {t('catalog.show_archived', { count: archivedCount })}
        </FilterToggle>
      )}
    </span>
  );
}

/** How the catalog's rows are grouped: not at all, by team, namespace, tier, or the value of a tag. */
export function CatalogGroupingSelect({
  grouping,
  tagKeys,
  onChange,
}: {
  grouping: CatalogGrouping;
  tagKeys: readonly string[];
  onChange: (grouping: CatalogGrouping) => void;
}) {
  const { t } = useTranslation('apm');

  return (
    <FittedSelect
      aria-label={t('catalog.group.label')}
      data-apm-catalog-grouping=""
      value={grouping || NONE}
      onValueChange={(value) => onChange(value === NONE ? '' : (value as CatalogGrouping))}
      options={[
        { value: NONE, label: t('catalog.group.off') },
        ...(['team', 'namespace', 'tier'] as const).map((value) => ({ value, label: t(`catalog.group.by.${value}`) })),
        ...tagKeys.map((key) => ({ value: `tag:${key}`, label: t('catalog.group.by_tag', { tag: key }) })),
      ]}
      maxWidth={200}
      prefix={grouping ? t('catalog.group.label') : undefined}
      className={SELECT}
    />
  );
}
