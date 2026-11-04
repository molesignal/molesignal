import { http } from '@/lib/http';

export type DeploymentStatus = 'succeeded' | 'failed' | 'rolled_back';

/** What changed: a version deployed, the configuration, or a feature flag. */
export type ChangeKind = 'deployment' | 'configuration' | 'feature_flag';

/**
 * A deployment, or another change, as the pipeline or tool that made it
 * reported it. A namespace or environment left empty was not said: the
 * change is listed in every one. A change other than a deployment may name
 * no version.
 */
export interface DeploymentEvent {
  id: string;
  kind?: ChangeKind;
  namespace: string;
  service: string;
  environment: string;
  version: string;
  status: DeploymentStatus;
  commit?: string;
  /** The code's repository, its credentials masked. */
  repositoryUrl?: string;
  pullRequestUrl?: string;
  author?: string;
  ciUrl?: string;
  description?: string;
  deployedAt: number;
  createdBy: string;
  createdAt: number;
}

export interface DeploymentListParams {
  service?: string | undefined;
  namespace?: string | undefined;
  environment?: string | undefined;
  from?: number | undefined;
  to?: number | undefined;
  limit?: number | undefined;
}

export const apmDeploymentsApi = {
  list: async (params: DeploymentListParams) => {
    const { data } = await http.get<DeploymentEvent[]>('/apm/deployments', {
      params: Object.fromEntries(Object.entries(params).filter(([, value]) => value !== undefined && value !== '')),
    });
    return data;
  },
};

export const apmDeploymentsQueryKeys = {
  list: (orgId: string, params: DeploymentListParams) =>
    [
      'apm',
      'deployments',
      orgId,
      params.namespace ?? null,
      params.service ?? null,
      params.environment ?? null,
      params.from ?? null,
      params.to ?? null,
    ] as const,
};
