import { Bell, BellPlus } from 'lucide-react';

import type {
  ProductBreadcrumbItem,
  ProductEdition,
  ProductRouteMeta,
} from '../ia';

type RouteInput = Omit<ProductRouteMeta, 'edition'> &
  Partial<Pick<ProductRouteMeta, 'edition'>>;

export const ALERT_PRODUCT_ROUTES = [
  route({
    id: 'alerts',
    path: '/alerts',
    labelKey: 'alerts',
    group: 'reliability',
    icon: Bell,
    owner: 'alerts',
    emptyStateStrategy: 'create-first',
    nav: true,
  }),
  route({
    id: 'alert.incidents',
    path: '/alerts/incidents',
    labelKey: 'breadcrumbs.alert_incidents',
    group: 'reliability',
    icon: Bell,
    owner: 'alerts',
    emptyStateStrategy: 'query-first',
  }),
  route({
    id: 'alert.rules',
    path: '/alerts/rules',
    labelKey: 'breadcrumbs.alert_rules',
    group: 'reliability',
    icon: Bell,
    owner: 'alerts',
    emptyStateStrategy: 'create-first',
  }),
  route({
    id: 'alert.rule.new',
    path: '/alerts/rules/new',
    labelKey: 'breadcrumbs.alert_rule_new',
    group: 'reliability',
    icon: BellPlus,
    owner: 'alerts',
    emptyStateStrategy: 'create-first',
    breadcrumbs: [
      crumb('alerts', '/alerts/incidents'),
      crumb('breadcrumbs.alert_rules', '/alerts/rules'),
      crumb('breadcrumbs.alert_rule_new'),
    ],
    backTo: '/alerts/rules',
  }),
  route({
    id: 'alert.rule.edit',
    path: '/alerts/rules/:id/edit',
    labelKey: 'breadcrumbs.alert_rule_edit',
    group: 'reliability',
    icon: Bell,
    owner: 'alerts',
    emptyStateStrategy: 'create-first',
    breadcrumbs: [
      crumb('alerts', '/alerts/incidents'),
      crumb('breadcrumbs.alert_rules', '/alerts/rules'),
      crumb('breadcrumbs.alert_rule_edit'),
    ],
    backTo: '/alerts/rules',
  }),
  route({
    id: 'incident.detail',
    path: '/alerts/incidents/:id',
    labelKey: 'breadcrumbs.incident_detail',
    group: 'reliability',
    icon: Bell,
    owner: 'alerts',
    emptyStateStrategy: 'query-first',
    breadcrumbs: [crumb('alerts', '/alerts/incidents'), crumb('breadcrumbs.incident_detail')],
    backTo: '/alerts/incidents',
  }),
  route({
    id: 'alert.schedule.detail',
    path: '/alerts/schedules/:id',
    labelKey: 'breadcrumbs.alert_schedule_detail',
    group: 'reliability',
    icon: Bell,
    owner: 'alerts',
    emptyStateStrategy: 'query-first',
    breadcrumbs: [
      crumb('alerts', '/alerts/incidents'),
      crumb('breadcrumbs.alert_schedules', '/alerts/schedules'),
      crumb('breadcrumbs.alert_schedule_detail'),
    ],
  }),
  route({
    id: 'anomaly.new',
    path: '/alerts/anomaly/add',
    labelKey: 'breadcrumbs.anomaly_new',
    group: 'reliability',
    icon: BellPlus,
    owner: 'alerts',
    emptyStateStrategy: 'create-first',
    breadcrumbs: [crumb('alerts', '/alerts'), crumb('breadcrumbs.anomaly_new')],
    backTo: '/alerts',
  }),
  route({
    id: 'anomaly.edit',
    path: '/alerts/anomaly/edit/:id',
    labelKey: 'breadcrumbs.anomaly_edit',
    group: 'reliability',
    icon: Bell,
    owner: 'alerts',
    emptyStateStrategy: 'create-first',
    breadcrumbs: [crumb('alerts', '/alerts'), crumb('breadcrumbs.anomaly_edit')],
    backTo: '/alerts',
  }),
  route({
    id: 'semantic.groups',
    path: '/alerts/semantic-groups',
    labelKey: 'breadcrumbs.semantic_groups',
    group: 'reliability',
    icon: Bell,
    owner: 'alerts',
    emptyStateStrategy: 'create-first',
  }),
  route({
    id: 'import.semantic.groups',
    path: '/alerts/import-semantic-groups',
    labelKey: 'breadcrumbs.semantic_groups',
    group: 'reliability',
    icon: Bell,
    owner: 'alerts',
    emptyStateStrategy: 'create-first',
  }),
] as const satisfies readonly ProductRouteMeta[];

function route<const T extends RouteInput>(value: T): T & { edition: ProductEdition } {
  return { edition: 'any', ...value };
}

function crumb(labelKey: string, to?: string): ProductBreadcrumbItem {
  return to ? { labelKey, to } : { labelKey };
}
