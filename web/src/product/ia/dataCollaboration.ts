import {
  createLucideIcon,
  FileBarChart,
  FunctionSquare,
  Plug,
  TableProperties,
  Waves,
  Workflow,
  Zap,
} from 'lucide-react';

import type {
  ProductBreadcrumbItem,
  ProductEdition,
  ProductRouteMeta,
} from '../ia';
import { STATUS_PAGE_PRODUCT_ROUTES } from './statusPages';

const WorkflowPlus = createLucideIcon('WorkflowPlus', [
  ['rect', { width: '8', height: '8', x: '3', y: '3', rx: '2', key: 'source' }],
  ['path', { d: 'M7 11v4a2 2 0 0 0 2 2h4', key: 'flow' }],
  ['rect', { width: '8', height: '8', x: '13', y: '13', rx: '2', key: 'target' }],
  ['path', { d: 'M15.5 6h6', key: 'plus-horizontal' }],
  ['path', { d: 'M18.5 3v6', key: 'plus-vertical' }],
]);

type RouteInput = Omit<ProductRouteMeta, 'edition'> &
  Partial<Pick<ProductRouteMeta, 'edition'>>;

export const DATA_COLLABORATION_PRODUCT_ROUTES = [
  route({
    id: 'datasource', path: '/datasource', labelKey: 'datasource', group: 'data',
    icon: Plug, owner: 'datasource', emptyStateStrategy: 'activation', nav: true,
  }),
  route({
    id: 'datasource.category', path: '/datasource/:category',
    labelKey: 'breadcrumbs.datasource_category', group: 'data', icon: Plug,
    owner: 'datasource', emptyStateStrategy: 'activation',
    breadcrumbs: [crumb('datasource', '/datasource'), crumb('breadcrumbs.datasource_category')],
    backTo: '/datasource',
  }),
  route({
    id: 'datasource.source', path: '/datasource/:category/:source',
    labelKey: 'breadcrumbs.datasource_source', group: 'data', icon: Plug,
    owner: 'datasource', emptyStateStrategy: 'activation',
    breadcrumbs: [
      crumb('datasource', '/datasource'),
      crumb('breadcrumbs.datasource_category'),
      crumb('breadcrumbs.datasource_source'),
    ],
    backTo: '/datasource',
  }),
  route({
    id: 'streams', path: '/streams', labelKey: 'streams', group: 'data',
    icon: Waves, owner: 'streams', emptyStateStrategy: 'create-first', nav: true,
  }),
  route({
    id: 'stream.explore', path: '/streams/:id', labelKey: 'breadcrumbs.stream_explore',
    group: 'data', icon: Waves, owner: 'streams', emptyStateStrategy: 'query-first',
    breadcrumbs: [crumb('streams', '/streams'), crumb('breadcrumbs.stream_explore')],
    backTo: '/streams',
  }),
  route({
    id: 'pipelines', path: '/pipelines', labelKey: 'pipelines', group: 'data',
    icon: Workflow, owner: 'pipelines', emptyStateStrategy: 'create-first', nav: true,
  }),
  route({
    id: 'pipeline.detail', path: '/pipelines/:id', labelKey: 'breadcrumbs.pipeline_detail',
    group: 'data', icon: Workflow, owner: 'pipelines', emptyStateStrategy: 'create-first',
    breadcrumbs: [crumb('pipelines', '/pipelines'), crumb('breadcrumbs.pipeline_detail')],
    backTo: '/pipelines',
  }),
  route({
    id: 'pipeline.add', path: '/pipelines/new', labelKey: 'breadcrumbs.pipeline_add',
    group: 'data', icon: WorkflowPlus, owner: 'pipelines', emptyStateStrategy: 'create-first',
    breadcrumbs: [crumb('pipelines', '/pipelines'), crumb('breadcrumbs.pipeline_add')],
    backTo: '/pipelines',
  }),
  route({
    id: 'pipeline.import', path: '/pipelines/import', labelKey: 'breadcrumbs.pipeline_import',
    group: 'data', icon: Workflow, owner: 'pipelines', emptyStateStrategy: 'create-first',
    breadcrumbs: [crumb('pipelines', '/pipelines'), crumb('breadcrumbs.pipeline_import')],
    backTo: '/pipelines',
  }),
  route({
    id: 'pipeline.edit', path: '/pipelines/:id/edit', labelKey: 'breadcrumbs.pipeline_edit',
    group: 'data', icon: Workflow, owner: 'pipelines', emptyStateStrategy: 'create-first',
    breadcrumbs: [crumb('pipelines', '/pipelines'), crumb('breadcrumbs.pipeline_edit')],
    backTo: '/pipelines',
  }),
  route({
    id: 'pipeline.history', path: '/pipelines/:id/history',
    labelKey: 'breadcrumbs.pipeline_history', group: 'data', icon: Workflow,
    owner: 'pipelines', emptyStateStrategy: 'query-first',
    breadcrumbs: [crumb('pipelines', '/pipelines'), crumb('breadcrumbs.pipeline_history')],
    backTo: '/pipelines',
  }),
  route({
    id: 'pipeline.backfill', path: '/pipelines/:id/backfill',
    labelKey: 'breadcrumbs.pipeline_backfill', group: 'data', icon: Zap,
    owner: 'automation', emptyStateStrategy: 'create-first',
    breadcrumbs: [crumb('pipelines', '/pipelines'), crumb('breadcrumbs.pipeline_backfill')],
    backTo: '/pipelines',
  }),
  route({
    id: 'functions', path: '/functions', labelKey: 'functions', group: 'data',
    icon: FunctionSquare, owner: 'functions', emptyStateStrategy: 'create-first', nav: true,
  }),
  route({
    id: 'extend.tables', path: '/extend-tables', labelKey: 'extend_tables', group: 'data',
    icon: TableProperties, owner: 'functions', emptyStateStrategy: 'create-first', nav: true,
  }),
  route({
    id: 'extend.table.detail', path: '/extend-tables/:table',
    labelKey: 'breadcrumbs.extend_table_detail', group: 'data', icon: TableProperties,
    owner: 'functions', emptyStateStrategy: 'query-first',
    breadcrumbs: [crumb('extend_tables', '/extend-tables'), crumb('breadcrumbs.extend_table_detail')],
    backTo: '/extend-tables',
  }),
  route({
    id: 'reports', path: '/reports', labelKey: 'reports', group: 'collaboration',
    icon: FileBarChart, owner: 'reports', emptyStateStrategy: 'create-first', nav: true,
  }),
  ...STATUS_PAGE_PRODUCT_ROUTES,
] as const satisfies readonly ProductRouteMeta[];

function route<const T extends RouteInput>(value: T): T & { edition: ProductEdition } {
  return { edition: 'any', ...value };
}

function crumb(labelKey: string, to?: string): ProductBreadcrumbItem {
  return to ? { labelKey, to } : { labelKey };
}
