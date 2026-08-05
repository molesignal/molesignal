import type { FromSchema } from 'json-schema-to-ts';

import type { DashboardDefinition } from '../schema';
import {
  dashboardAuthoringV1Schema,
  dashboardModelV2Schema,
} from './generated/schemas';

// json-schema-to-ts cannot safely expand the recursive Dashboard element graph.
// Derive the scalar envelope from the generated schema and reuse the renderer's
// recursive domain types; Ajv remains authoritative for every nested boundary.
const _dashboardModelEnvelopeSchema = {
  type: 'object',
  additionalProperties: false,
  required: [
    'engine',
    'schemaVersion',
    'uid',
    'title',
    'tags',
    'editable',
    'defaultDashboard',
  ],
  properties: {
    engine: dashboardModelV2Schema.properties.engine,
    schemaVersion: dashboardModelV2Schema.properties.schemaVersion,
    uid: dashboardModelV2Schema.$defs.nonEmptyString,
    title: dashboardModelV2Schema.$defs.nonEmptyString,
    tags: dashboardModelV2Schema.properties.tags,
    editable: dashboardModelV2Schema.properties.editable,
    defaultDashboard: dashboardModelV2Schema.properties.defaultDashboard,
  },
} as const;

type DashboardModelEnvelope = FromSchema<
  typeof _dashboardModelEnvelopeSchema
>;
type DashboardModelRuntimeCore = Pick<
  DashboardDefinition,
  | 'timeSettings'
  | 'refreshSettings'
  | 'interactionSettings'
  | 'variables'
  | 'annotations'
  | 'links'
  | 'layout'
  | 'elements'
>;
type DashboardModelServerFields = Pick<
  DashboardDefinition,
  | 'id'
  | 'description'
  | 'folderId'
  | 'version'
  | 'createdAt'
  | 'updatedAt'
  | 'createdBy'
  | 'updatedBy'
>;

export type DashboardModelV2Contract = DashboardModelEnvelope &
  DashboardModelRuntimeCore &
  Partial<DashboardModelServerFields> & {
    extensions?: Record<string, unknown> | undefined;
  };

const _dashboardAuthoringEnvelopeSchema = {
  type: 'object',
  additionalProperties: false,
  required: ['authoringVersion', 'title'],
  properties: {
    authoringVersion:
      dashboardAuthoringV1Schema.properties.authoringVersion,
    title: dashboardAuthoringV1Schema.$defs.title,
    description: dashboardAuthoringV1Schema.properties.description,
    tags: dashboardAuthoringV1Schema.properties.tags,
    folderId: dashboardAuthoringV1Schema.$defs.name,
  },
} as const;

type DashboardAuthoringEnvelope = FromSchema<
  typeof _dashboardAuthoringEnvelopeSchema
>;

export type DashboardAuthoringV1Contract = DashboardAuthoringEnvelope & {
  timeRange?:
    | { from: string; to: string; timezone?: string | undefined }
    | undefined;
  refresh?:
    | { mode: 'off' }
    | { mode: 'interval'; interval: string }
    | { mode: 'live' }
    | undefined;
  variables?: Array<Record<string, unknown>> | undefined;
  elements: Array<Record<string, unknown>>;
};
