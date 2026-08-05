import { nanoid } from 'nanoid';

import { layoutBottom } from './layout';
import { QUERY_LEGEND_AUTO } from './query/legend';
import type {
  DashboardElement,
  DashboardGroup,
  DashboardPanel,
  DashboardRow,
  DashboardTab,
  DashboardTextElement,
  GridPosition,
  VisualizationType,
} from './schema';
import { visualizationRegistry } from './visualizations';

export function createDashboardPanel(
  elements: readonly DashboardElement[] = [],
  visualization: VisualizationType = 'time_series',
): DashboardPanel {
  return {
    kind: 'panel',
    id: `panel-${nanoid(10)}`,
    title: 'New panel',
    gridPos: nextPosition(elements, 12, 24),
    queryOptions: {},
    queries: [
      {
        refId: 'A',
        enabled: true,
        dataSourceType: 'metrics',
        legend: QUERY_LEGEND_AUTO,
        query: {
          language: 'promql',
          expression: '',
        },
      },
    ],
    transformations: [],
    visualization: {
      type: visualization,
      schemaVersion: 1,
      options: {
        ...visualizationRegistry.get(visualization).defaultOptions,
      },
    },
    fieldConfig: {},
    overrides: [],
    links: [],
  };
}

export function createDashboardText(
  elements: readonly DashboardElement[] = [],
): DashboardTextElement {
  return {
    kind: 'text',
    id: `text-${nanoid(10)}`,
    title: 'Text',
    gridPos: nextPosition(elements, 12, 12),
    content: '',
    mode: 'markdown',
  };
}

export function createDashboardRow(
  elements: readonly DashboardElement[] = [],
): DashboardRow {
  return {
    kind: 'row',
    id: `row-${nanoid(10)}`,
    title: 'New row',
    gridPos: nextPosition(elements, 24, 28),
    collapsed: false,
    elements: [],
  };
}

export function createDashboardGroup(
  elements: readonly DashboardElement[] = [],
): DashboardGroup {
  return {
    kind: 'group',
    id: `group-${nanoid(10)}`,
    title: 'New group',
    gridPos: nextPosition(elements, 24, 28),
    collapsed: false,
    elements: [],
  };
}

export function createDashboardTab(
  elements: readonly DashboardElement[] = [],
): DashboardTab {
  const tabId = `tab-item-${nanoid(8)}`;
  return {
    kind: 'tab',
    id: `tabs-${nanoid(10)}`,
    title: 'New tabs',
    gridPos: nextPosition(elements, 24, 28),
    defaultTabId: tabId,
    tabs: [
      {
        id: tabId,
        title: 'Tab 1',
        elements: [],
      },
    ],
  };
}

export function duplicateDashboardElement(
  element: DashboardElement,
  offset = 1,
  copySuffix = 'copy',
): DashboardElement {
  const copy = globalThis.structuredClone(element);
  renewIds(copy);
  copy.gridPos = {
    ...copy.gridPos,
    x: copy.gridPos.x + offset,
    y: copy.gridPos.y + offset,
  };
  copy.title = `${copy.title} ${copySuffix}`;
  return copy;
}

function nextPosition(
  elements: readonly DashboardElement[],
  w: number,
  h: number,
): GridPosition {
  return {
    x: 0,
    y: layoutBottom(elements),
    w,
    h,
    minW: 2,
    minH: 4,
  };
}

function renewIds(element: DashboardElement): void {
  element.id = `${element.kind}-${nanoid(10)}`;
  if (element.kind === 'group' || element.kind === 'row') {
    element.elements.forEach(renewIds);
  } else if (element.kind === 'tab') {
    for (const tab of element.tabs) {
      tab.id = `tab-item-${nanoid(8)}`;
      tab.elements.forEach(renewIds);
    }
    element.defaultTabId = element.tabs[0]?.id;
  }
}
