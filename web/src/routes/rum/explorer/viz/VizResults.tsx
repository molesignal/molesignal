import { useTranslation } from 'react-i18next';

import { ProductState } from '@/product/states';
import { QueryRecoveryState, type QueryRecoveryActions, type QueryRecoveryCopy } from '@/shell/query/RecoveryState';
import { usePreloadWorldMap } from '@/viz/worldmap/preload';

import { columnLabel } from '../labels';
import type { RumTab } from '../model';
import { RumAggregateTable } from './AggregateTable';
import { RumDistributionChart } from './Distribution';
import { measureLabel, type VizFilters, vizTitle } from './labels';
import { COUNTRY_FIELD } from './spec';
import { RumTimeseries } from './Timeseries';
import { RumTopList } from './TopList';
import type { RumVizModel } from './useRumVizState';
import { RumVizMap } from './VizMap';

/**
 * What stands under the way of drawing: the numbers drawn as it says, or why there are none. An
 * answer that could not be counted in time says so and what to do, and does not wait.
 */
export function RumVizResults({
  model,
  tab,
  language,
  filters,
  recovery,
  recoveryCopy,
  onRangeSelect,
}: {
  model: RumVizModel;
  tab: RumTab;
  language: string;
  filters: VizFilters;
  recovery: QueryRecoveryActions;
  recoveryCopy: QueryRecoveryCopy;
  onRangeSelect: (range: { from: number; to: number }) => void;
}) {
  const { t } = useTranslation('rum');
  const { spec, request, query, distribution } = model;
  const answer = query.data;
  const none = t('explorer.viz.no_value');

  // The shapes of a map load while its countries are being counted.
  usePreloadWorldMap(spec?.viz === 'map');

  const state = (kind: 'loading' | 'empty' | 'error') => (
    <QueryRecoveryState state={kind} error={query.error} copy={recoveryCopy} recovery={recovery} className="h-full min-h-0" testId="rum-viz-state" />
  );

  if (!spec || !request) return state('loading');

  if (spec.viz === 'distribution') {
    if (distribution.isError) {
      return <QueryRecoveryState state="error" error={distribution.error} copy={recoveryCopy} recovery={recovery} className="h-full min-h-0" testId="rum-viz-state" />;
    }

    if (!spec.field || !distribution.data) return state(spec.field ? 'loading' : 'empty');

    if (distribution.data.unavailable) {
      return (
        <div data-rum-viz-unavailable={distribution.data.unavailable} className="flex min-h-0 flex-1 flex-col">
          <ProductState variant="empty" title={t(`overview.unavailable.${distribution.data.unavailable}`)} description="" />
        </div>
      );
    }

    if (distribution.data.rows === 0) return state('empty');

    return <RumDistributionChart answer={distribution.data} title={t('explorer.viz.distribution.title', { field: columnLabel(t, spec.field) })} language={language} />;
  }

  if (query.isError) return state('error');
  if (!answer) return state('loading');

  if (answer.unavailable) {
    return (
      <div data-rum-viz-unavailable={answer.unavailable} className="flex min-h-0 flex-1 flex-col">
        <ProductState variant="empty" title={t(`overview.unavailable.${answer.unavailable}`)} description="" />
      </div>
    );
  }

  if (answer.rows.length === 0) return state('empty');

  const measure = spec.measures[0]!;
  const name = measureLabel(t, measure, tab);
  const title = vizTitle(t, name, spec.viz === 'map' ? [COUNTRY_FIELD] : spec.groups);

  switch (spec.viz) {
    case 'timeseries':
      return request.bars ? (
        <RumTimeseries
          answer={answer}
          bars={request.bars}
          spec={spec}
          title={title}
          measureName={name}
          language={language}
          none={none}
          filters={filters}
          onRangeSelect={onRangeSelect}
        />
      ) : null;
    case 'toplist':
      return <RumTopList answer={answer} spec={spec} title={title} language={language} none={none} filters={filters} />;
    case 'table':
      return (
        <RumAggregateTable
          answer={answer}
          spec={spec}
          groupHeads={spec.groups.map((field) => columnLabel(t, field))}
          measureHeads={spec.measures.map((entry) => measureLabel(t, entry, tab))}
          language={language}
          none={none}
          filters={filters}
          onSort={(by, descending) => model.onSettingsChange({ sort: by, descending })}
        />
      );
    case 'map':
      return <RumVizMap answer={answer} title={title} measureName={name} language={language} />;
  }
}

