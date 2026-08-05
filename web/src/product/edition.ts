import { useQuery } from '@tanstack/react-query';
import { create } from 'zustand';

import * as billingApi from '@/api/billing';
import * as licenseApi from '@/api/license';
import type { LicenseSnapshot } from '@/api/license';
import { hasPermission, useProductAccess } from '@/product/access';
import type { PermissionKey } from '@/product/permissions';

export type DeploymentMode = 'oss' | 'pro' | 'saas';
export type TrialState = 'none' | 'active' | 'expired';
export type FeatureKey =
  | 'intelligence'
  | 'domain-management'
  | 'federated-search'
  | 'saas-billing'
  | 'saas-support';
export type FeatureGateStatus =
  | 'loading'
  | 'allowed'
  | 'backend-pending'
  | 'permission-denied'
  | 'pro-required'
  | 'saas-only'
  | 'trial-available';

export interface EditionState {
  deploymentMode: DeploymentMode | null;
  trialState: TrialState;
  backendPendingFeatures: readonly FeatureKey[];
  setDeploymentMode: (mode: DeploymentMode | null) => void;
  setTrialState: (state: TrialState) => void;
  setBackendPendingFeatures: (features: readonly FeatureKey[]) => void;
}

export interface FeatureDefinition {
  key: FeatureKey;
  labelKey: string;
  licenseFeature?: string;
  deployments?: readonly DeploymentMode[];
  requiredPermission?: PermissionKey;
  backendPending?: boolean;
  trialAvailable?: boolean;
}

export interface EditionMetadata {
  deploymentMode: DeploymentMode;
  trialState: TrialState;
  permissions: ReadonlySet<PermissionKey>;
  features: ReadonlySet<string>;
  backendPendingFeatures: ReadonlySet<FeatureKey>;
  license: LicenseSnapshot | null;
  loaded: boolean;
}

export interface FeatureGateResult {
  feature: FeatureDefinition;
  status: FeatureGateStatus;
  metadata: EditionMetadata;
}

export const FEATURE_DEFINITIONS: Record<FeatureKey, FeatureDefinition> = {
  intelligence: {
    key: 'intelligence',
    labelKey: 'features.intelligence',
    licenseFeature: 'intelligence',
    deployments: ['pro', 'saas'],
    requiredPermission: 'intelligence.manage',
    trialAvailable: true,
  },
  'domain-management': {
    key: 'domain-management',
    labelKey: 'features.domain_management',
    licenseFeature: 'domain_management',
    deployments: ['pro', 'saas'],
    requiredPermission: 'org.settings.manage',
  },
  'federated-search': {
    key: 'federated-search',
    labelKey: 'features.federated_search',
    licenseFeature: 'federated_search',
    deployments: ['pro', 'saas'],
    requiredPermission: 'org.settings.manage',
  },
  'saas-billing': {
    key: 'saas-billing',
    labelKey: 'features.saas_billing',
    deployments: ['saas'],
    requiredPermission: 'org.billing.read',
  },
  'saas-support': {
    key: 'saas-support',
    labelKey: 'features.saas_support',
    deployments: ['saas'],
    requiredPermission: 'org.settings.read',
  },
};

export const useEditionStore = create<EditionState>((set) => ({
  deploymentMode: null,
  trialState: 'none',
  backendPendingFeatures: [],
  setDeploymentMode: (deploymentMode) => set({ deploymentMode }),
  setTrialState: (trialState) => set({ trialState }),
  setBackendPendingFeatures: (backendPendingFeatures) => set({ backendPendingFeatures }),
}));

export function normalizeEditionMetadata(args: {
  license?: LicenseSnapshot | null;
  licenseLoaded?: boolean;
  deploymentMode?: DeploymentMode | null;
  trialState?: TrialState;
  permissions?: readonly PermissionKey[];
  backendPendingFeatures?: readonly FeatureKey[];
}): EditionMetadata {
  const license = args.license ?? null;
  const licenseImpliesPro =
    license?.edition === 'pro' && license.verified && !license.expired;
  const deploymentMode = args.deploymentMode ?? (licenseImpliesPro ? 'pro' : 'oss');

  return {
    deploymentMode,
    trialState: args.trialState ?? 'none',
    permissions: new Set(args.permissions ?? []),
    features: new Set(license?.features ?? []),
    backendPendingFeatures: new Set(args.backendPendingFeatures ?? []),
    license,
    loaded: args.licenseLoaded ?? license !== null,
  };
}

export function selectFeatureGate(
  metadata: EditionMetadata,
  feature: FeatureKey | FeatureDefinition,
): FeatureGateResult {
  const definition = typeof feature === 'string' ? FEATURE_DEFINITIONS[feature] : feature;

  // License feature availability remains server-authoritative. The two
  // account-only SaaS surfaces are the exception: their entries must not look
  // usable on self-hosted deployments, even when a role has the matching
  // organization permission.
  if (!metadata.loaded && definition.licenseFeature) {
    return { feature: definition, status: 'loading', metadata };
  }
  if (definition.backendPending || metadata.backendPendingFeatures.has(definition.key)) {
    return { feature: definition, status: 'backend-pending', metadata };
  }
  if (
    definition.requiredPermission &&
    !metadata.permissions.has(definition.requiredPermission)
  ) {
    return { feature: definition, status: 'permission-denied', metadata };
  }
  if (
    definition.deployments?.length === 1 &&
    definition.deployments[0] === 'saas' &&
    metadata.deploymentMode !== 'saas'
  ) {
    return { feature: definition, status: 'saas-only', metadata };
  }

  return { feature: definition, status: 'allowed', metadata };
}

export function useEditionMetadata(): EditionMetadata {
  const deploymentMode = useEditionStore((s) => s.deploymentMode);
  const storeTrialState = useEditionStore((s) => s.trialState);
  const backendPendingFeatures = useEditionStore((s) => s.backendPendingFeatures);
  const access = useProductAccess();
  const canReadLicense = hasPermission('sys.licenses.read', access);
  const q = useQuery({
    queryKey: ['license-snapshot'],
    queryFn: () => licenseApi.get(),
    enabled: canReadLicense,
    staleTime: 60_000,
    retry: false,
  });
  // Trial state is sourced from the backend (org_trials); falls back to the
  // store value while the request is in flight. `converted` maps to `none`
  // (a paid org shows no trial UI).
  const trialQuery = useQuery({
    queryKey: ['billing-trial'],
    queryFn: () => billingApi.getTrial(),
    enabled: access?.scope === 'organization',
    staleTime: 60_000,
    retry: false,
  });
  const trialState: TrialState = trialQuery.data
    ? trialQuery.data.state === 'active'
      ? 'active'
      : trialQuery.data.state === 'expired'
        ? 'expired'
        : 'none'
    : storeTrialState;

  return normalizeEditionMetadata({
    license: canReadLicense ? (q.data ?? null) : null,
    // Tenant sessions never request or retain License metadata. Feature
    // availability is server-authoritative, so a hidden License is considered
    // loaded here to avoid turning tenant pages into an infinite loading gate.
    licenseLoaded: !canReadLicense || q.data !== undefined || q.isError,
    deploymentMode,
    trialState,
    permissions: access ? [...access.permissions] : [],
    backendPendingFeatures,
  });
}

export function useFeatureGate(feature: FeatureKey): FeatureGateResult {
  return selectFeatureGate(useEditionMetadata(), feature);
}
