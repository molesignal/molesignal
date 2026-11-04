import type { DependencyKind } from '@/api/statusPages';

export const DEPENDENCY_KINDS: readonly DependencyKind[] = [
  'technical_service',
  'synthetic_monitor',
  'alert_rule',
];

/** Where the thing a dependency names is looked at in the product. */
export function dependencyHref(kind: DependencyKind, target: string): string {
  const id = encodeURIComponent(target);
  switch (kind) {
    case 'technical_service':
      return `/apm/services/${id}`;
    case 'synthetic_monitor':
      return `/synthetics/checks/${id}`;
    case 'alert_rule':
      return `/alerts/rules/${id}/edit`;
  }
}
