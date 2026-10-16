import { useQuery } from '@tanstack/react-query';

import * as alertsApi from '@/api/alerts';
import { apmApi } from '@/api/apm';
import * as businessServicesApi from '@/api/businessServices';
import type { BusinessServiceDependencyKind } from '@/api/businessServices';
import * as syntheticsApi from '@/api/synthetics';
import { useActionAccess } from '@/product/actionAccess';

export interface DependencyTargetOption {
  value: string;
  label: string;
}

/**
 * What a dependency can name, kind by kind, and how a named one is called:
 * alert rules, monitors and business services by their names, technical
 * services by the names traces and alerts carry, offered from APM when it has
 * seen them. A list that cannot be read leaves its targets named by id.
 */
export function useDependencyTargets(enabled: boolean, excludeServiceId?: string) {
  const alertAccess = useActionAccess({ permission: 'alerts.read' });
  const syntheticsAccess = useActionAccess({ permission: 'synthetics.read' });
  const rules = useQuery({
    queryKey: ['alerts', 'rules'],
    queryFn: alertsApi.list,
    enabled: enabled && alertAccess.allowed,
    staleTime: 30_000,
  });
  const monitors = useQuery({
    queryKey: ['synthetics', 'monitors'],
    queryFn: syntheticsApi.listMonitors,
    enabled: enabled && syntheticsAccess.allowed,
    staleTime: 30_000,
  });
  const services = useQuery({
    queryKey: ['business-services'],
    queryFn: businessServicesApi.list,
    enabled,
    staleTime: 30_000,
  });
  const technical = useQuery({
    queryKey: ['business-services', 'technical-services'],
    queryFn: () => apmApi.services({ limit: 200 }),
    enabled,
    staleTime: 60_000,
    retry: false,
  });

  const byKind: Record<BusinessServiceDependencyKind, DependencyTargetOption[]> = {
    technical_service: uniqueNames(apmServiceNames(technical.data))
      .map((name) => ({ value: name, label: name })),
    synthetic_monitor: (monitors.data ?? [])
      .filter((monitor) => monitor.lifecycle !== 'archived')
      .map((monitor) => ({ value: monitor.id, label: monitor.name })),
    alert_rule: (rules.data ?? []).map((rule) => ({ value: rule.id, label: rule.name })),
    business_service: (services.data ?? [])
      .filter((service) => service.id !== excludeServiceId)
      .map((service) => ({ value: service.id, label: service.name })),
  };

  for (const options of Object.values(byKind)) {
    options.sort((left, right) => left.label.localeCompare(right.label));
  }

  return {
    options: (kind: BusinessServiceDependencyKind) => byKind[kind],
    /** The name a target goes by; a technical service is its own name. */
    label: (kind: BusinessServiceDependencyKind, target: string) =>
      kind === 'technical_service'
        ? target
        : byKind[kind].find((option) => option.value === target)?.label ?? target,
    loading: rules.isLoading || monitors.isLoading || services.isLoading,
  };
}

/**
 * The service names an APM reply offers. They are only suggestions, so a
 * reply of another shape offers none rather than breaking the page.
 */
function apmServiceNames(reply: unknown): string[] {
  const items = (reply as { items?: unknown } | null | undefined)?.items;
  if (!Array.isArray(items)) return [];

  return items.flatMap((item: { service?: { name?: unknown } } | null) => {
    const name = item?.service?.name;
    return typeof name === 'string' ? [name] : [];
  });
}

function uniqueNames(names: string[]): string[] {
  return [...new Set(names.filter(Boolean))];
}
