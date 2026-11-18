import { ChartBarDecreasing, ChartColumn, ChartLine, Earth, List, type LucideIcon, Table } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import { QueryToolbarButton, QueryToolbarGroup } from '@/shell/query/Workbench';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';
import { preloadWorldMap } from '@/viz/worldmap/preload';

import { RUM_VIZ, type RumViz } from './address';

/** The sign of each way of drawing the rows. */
const ICON: Record<RumViz, LucideIcon> = {
  list: List,
  timeseries: ChartLine,
  toplist: ChartBarDecreasing,
  table: Table,
  map: Earth,
  distribution: ChartColumn,
};

/**
 * How the rows the query matches are drawn: as the rows themselves, or as numbers over them, over
 * time, ranked, in a table, on a map or as how a number falls. A row of icons at the end of the
 * results' head, beside the switch that splits the page, each naming itself in a tooltip. A map
 * needs where the clients were, which a type that has no country cannot give.
 */
export function RumVizSwitch({ viz, canMap, onChange }: { viz: RumViz; canMap: boolean; onChange: (viz: RumViz) => void }) {
  const { t } = useTranslation('rum');

  return (
    <QueryToolbarGroup
      role="group"
      aria-label={t('explorer.viz.label')}
      data-rum-viz-switch={viz}
      className="h-8 border-0 bg-[var(--control-surface)]"
    >
      {RUM_VIZ.map((candidate) => {
        const unavailable = candidate === 'map' && !canMap;
        const name = t(`explorer.viz.names.${candidate}`);
        const Icon = ICON[candidate];

        return (
          <Tooltip key={candidate}>
            <TooltipTrigger asChild>
              <QueryToolbarButton
                flat
                active={candidate === viz}
                aria-pressed={candidate === viz}
                aria-label={name}
                disabled={unavailable}
                title={unavailable ? t('explorer.viz.map_unavailable') : undefined}
                data-rum-viz={candidate}
                className="h-7 w-7 px-0"
                onClick={() => onChange(candidate)}
                // The shapes of the map start loading as the pointer or the keys come to its button.
                {...(candidate === 'map' ? { onPointerEnter: preloadWorldMap, onFocus: preloadWorldMap } : {})}
              >
                <Icon aria-hidden="true" className="h-3.5 w-3.5" />
              </QueryToolbarButton>
            </TooltipTrigger>
            <TooltipContent>{name}</TooltipContent>
          </Tooltip>
        );
      })}
    </QueryToolbarGroup>
  );
}
