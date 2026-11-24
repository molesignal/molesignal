import { http } from '@/lib/http';

import type { ServiceIdentity } from './apm';

/** What services run on. */
export type EntityKind = 'cluster' | 'node' | 'host' | 'pod' | 'container';

export const ENTITY_KINDS: readonly EntityKind[] = ['cluster', 'node', 'host', 'pod', 'container'];

/**
 * A cluster, node, host, pod or container seen in the resource fields of
 * spans, metrics or logs. A node's key is `cluster/node`, a pod's
 * `cluster/namespace/pod`, a host's its name, a container's its id.
 */
export interface Entity {
  kind: EntityKind;
  key: string;
  name: string;
  attributes: Record<string, string>;
  firstSeenAt: number;
  lastSeenAt: number;
}

/** An entity in a list, with how many services run on it directly. */
export interface EntityRow extends Entity {
  services: number;
}

/** An entity a link leads to. */
export interface EntityNeighbour {
  kind: EntityKind;
  key: string;
  name: string;
}

export interface EntityDetail {
  entity: Entity;
  /** What it runs on or is part of. */
  parents: EntityNeighbour[];
  /** What runs on or is part of it. */
  children: EntityNeighbour[];
  /** The services on it, and on what runs on it. */
  services: ServiceIdentity[];
}

export interface EntityListParams {
  kind?: EntityKind;
  search?: string;
  seenSince?: number;
  limit?: number;
}

export const entityQueryKeys = {
  all: (orgId: string) => ['entities', orgId] as const,
  list: (orgId: string, params: EntityListParams) => ['entities', orgId, 'list', params] as const,
  detail: (orgId: string, kind: string, key: string) => ['entities', orgId, 'detail', kind, key] as const,
};

export const entitiesApi = {
  list: async (params: EntityListParams = {}) => {
    const { data } = await http.get<EntityRow[]>('/entities', {
      params: {
        ...(params.kind ? { kind: params.kind } : {}),
        ...(params.search?.trim() ? { search: params.search.trim() } : {}),
        ...(params.seenSince !== undefined ? { seenSince: params.seenSince } : {}),
        ...(params.limit !== undefined ? { limit: params.limit } : {}),
      },
    });
    return Array.isArray(data) ? data : [];
  },
  detail: async (kind: string, key: string) => {
    const { data } = await http.get<EntityDetail>('/entities/detail', { params: { kind, key } });
    return data;
  },
};
