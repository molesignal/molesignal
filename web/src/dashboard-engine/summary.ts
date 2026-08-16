import type { Dashboard } from '@/types/dashboard';

import { dashboardDefinitionFromApi, flattenElements } from './model';

/** Count the visual elements presented as panels in dashboard list surfaces. */
export function dashboardPanelCount(dashboard: Dashboard): number {
  return flattenElements(dashboardDefinitionFromApi(dashboard).elements).filter(
    (element) => element.kind === 'panel' || element.kind === 'text',
  ).length;
}
