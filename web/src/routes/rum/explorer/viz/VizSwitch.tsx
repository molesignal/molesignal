import { useTranslation } from 'react-i18next';

import { QueryToolbarButton, QueryToolbarGroup } from '@/shell/query/Workbench';
import { preloadWorldMap } from '@/viz/worldmap/preload';

import { RUM_VIZ, type RumViz } from './address';

/**
 * How the rows the query matches are drawn: as the rows themselves, or as numbers over them, over
 * time, ranked, in a table or on a map. A map needs where the clients were, which a type that has
 * no country cannot give.
 */
export function RumVizSwitch({ viz, canMap, onChange }: { viz: RumViz; canMap: boolean; onChange: (viz: RumViz) => void }) {
  const { t } = useTranslation('rum');

  return (
    <div className="flex min-w-0 flex-wrap items-center gap-2">
      <span className="font-sans text-xs text-tx-3">{t('explorer.viz.label')}</span>
      <QueryToolbarGroup role="group" aria-label={t('explorer.viz.label')} data-rum-viz-switch={viz} className="border-0 bg-[var(--control-surface)]">
        {RUM_VIZ.map((candidate) => {
          const unavailable = candidate === 'map' && !canMap;

          return (
            <QueryToolbarButton
              key={candidate}
              flat
              active={candidate === viz}
              aria-pressed={candidate === viz}
              disabled={unavailable}
              title={unavailable ? t('explorer.viz.map_unavailable') : undefined}
              data-rum-viz={candidate}
              onClick={() => onChange(candidate)}
              // The shapes of the map start loading as the pointer or the keys come to its button.
              {...(candidate === 'map' ? { onPointerEnter: preloadWorldMap, onFocus: preloadWorldMap } : {})}
            >
              {t(`explorer.viz.names.${candidate}`)}
            </QueryToolbarButton>
          );
        })}
      </QueryToolbarGroup>
    </div>
  );
}
