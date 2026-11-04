import type { RedSummary, SparkPoint, TransactionIdentity } from './apm';

/** A value some of a service's instances share, and how many. */
export interface ProfileValue {
  value: string;
  instances: number;
}

/**
 * What a service runs as within the range, read from its instances (those of
 * the version in view when one is): each figure ranked by how many share it.
 */
export interface ServiceProfile {
  instanceCount: number;
  hostCount: number;
  versions: ProfileValue[];
  languages: ProfileValue[];
  runtimes: ProfileValue[];
  frameworks: ProfileValue[];
  regions: ProfileValue[];
  clusters: ProfileValue[];
  k8sNamespaces: ProfileValue[];
  workloads: ProfileValue[];
  repositories: ProfileValue[];
  commits: ProfileValue[];
  /** The kinds of entry the service serves. */
  entryKinds: TransactionIdentity['kind'][];
}

/**
 * One instance of a service: the version it last ran, what its resource says,
 * and what it served in the range. The instances past the limit are counted
 * together in the row of id `__other__`.
 */
export interface ServiceInstance {
  instanceId: string;
  version?: string | null;
  firstSeenAt: number;
  lastSeenAt: number;
  attributes: Record<string, string>;
  /** Older backends answer without what each instance served. */
  red?: RedSummary;
  sparkline?: SparkPoint[];
}
