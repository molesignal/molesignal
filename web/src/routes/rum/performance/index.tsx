import { PerformanceFrame } from './Frame';
import { LongTasksView } from './longtasks';
import { ResourcesView } from './resources';
import { VitalsView } from './Vitals';

/**
 * The performance tab: how the pages of an application do for the people who open them. Its first
 * view is the Web Vitals; the others are told by the resources the pages load and the long tasks
 * that block them.
 */
export function RumPerformance() {
  return <PerformanceFrame vitals>{(context) => <VitalsView context={context} />}</PerformanceFrame>;
}

export function RumPerformanceResources() {
  return <PerformanceFrame>{(context) => <ResourcesView context={context} />}</PerformanceFrame>;
}

export function RumPerformanceLongTasks() {
  return <PerformanceFrame>{(context) => <LongTasksView context={context} />}</PerformanceFrame>;
}
