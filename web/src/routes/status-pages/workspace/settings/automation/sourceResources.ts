import type { StatusPageComponent } from '@/api/statusPages';
import { monitorTarget, type MonitorDetail, type SyntheticMonitor } from '@/api/synthetics';
import type { AlertRule } from '@/types/alerting';

export interface AutomationSourceResource {
  id: string;
  name: string;
  target: string;
  state: 'enabled' | 'disabled' | 'active' | 'paused';
}

export function alertRuleResources(rules: AlertRule[]): AutomationSourceResource[] {
  return sortResources(rules.map((rule) => ({
    id: rule.id,
    name: rule.name,
    target: alertRuleTarget(rule),
    state: rule.enabled ? 'enabled' : 'disabled',
  })));
}

export function syntheticMonitorResources(
  monitors: SyntheticMonitor[],
  details: MonitorDetail[],
): AutomationSourceResource[] {
  const detailByMonitor = new Map(details.map((detail) => [detail.monitor.id, detail]));
  return sortResources(monitors.flatMap((monitor) => {
    if (
      !monitor.activeRevisionId
      || (monitor.lifecycle !== 'active' && monitor.lifecycle !== 'paused')
    ) {
      return [];
    }
    const detail = detailByMonitor.get(monitor.id);
    const revision = detail?.revisions.find((item) => item.id === monitor.activeRevisionId);
    return [{
      id: monitor.id,
      name: monitor.name,
      target: monitorTarget(revision?.spec),
      state: monitor.lifecycle,
    }];
  }));
}

/**
 * The page's components that depend on something, with the health worked out
 * from their dependencies where the others have a target; empty until it is.
 */
export function componentDependencyResources(components: StatusPageComponent[]): AutomationSourceResource[] {
  return sortResources(components.flatMap((component) =>
    component.lifecycle === 'active' && component.dependencies?.length
      ? [{
          id: component.id,
          name: component.name,
          target: component.dependencyHealth?.status ?? '',
          state: 'active' as const,
        }]
      : []));
}

function alertRuleTarget(rule: AlertRule): string {
  const stream = rule.query.stream;
  if (stream) return `${stream.streamType} / ${stream.name}`;
  return `${rule.query.language.toUpperCase()} / ${rule.kind ?? 'scheduled'}`;
}

function sortResources(resources: AutomationSourceResource[]): AutomationSourceResource[] {
  return resources.sort((left, right) => left.name.localeCompare(right.name));
}
