import { describe, expect, it } from 'vitest';

import type { LicenseSnapshot } from '@/api/license';

import { normalizeEditionMetadata, selectFeatureGate } from './edition';

const proLicense: LicenseSnapshot = {
  edition: 'pro',
  verified: true,
  expired: false,
  issued_to: 'Example',
  features: ['federated_search'],
  max_ingest_bytes_per_day: null,
  expires_at_micros: null,
  active_version_id: 'license-version-1',
};

describe('edition metadata', () => {
  it('falls back to OSS while leaving product features frontend-available', () => {
    const metadata = normalizeEditionMetadata({
      licenseLoaded: true,
      permissions: ['org.settings.manage'],
    });

    expect(metadata.deploymentMode).toBe('oss');
    expect(selectFeatureGate(metadata, 'federated-search').status).toBe('allowed');
  });

  it('allows licensed pro features for permitted roles', () => {
    const metadata = normalizeEditionMetadata({
      license: proLicense,
      licenseLoaded: true,
      permissions: ['org.settings.manage'],
    });

    expect(metadata.deploymentMode).toBe('pro');
    expect(selectFeatureGate(metadata, 'federated-search').status).toBe('allowed');
  });

  it('returns permission gates before license gates for roles that cannot manage a feature', () => {
    const metadata = normalizeEditionMetadata({
      license: proLicense,
      licenseLoaded: true,
      permissions: [],
    });

    expect(selectFeatureGate(metadata, 'federated-search').status).toBe('permission-denied');
  });

  it('keeps SaaS account surfaces gated while leaving trial features server-authoritative', () => {
    const selfHostedMetadata = normalizeEditionMetadata({
      deploymentMode: 'pro',
      licenseLoaded: true,
      permissions: ['org.billing.read'],
    });
    const saasMetadata = normalizeEditionMetadata({
      deploymentMode: 'saas',
      licenseLoaded: true,
      permissions: ['org.billing.read'],
    });
    const trialMetadata = normalizeEditionMetadata({
      deploymentMode: 'pro',
      trialState: 'active',
      licenseLoaded: true,
      permissions: ['intelligence.manage'],
    });

    expect(selectFeatureGate(selfHostedMetadata, 'saas-billing').status).toBe('saas-only');
    expect(selectFeatureGate(saasMetadata, 'saas-billing').status).toBe('allowed');
    expect(selectFeatureGate(trialMetadata, 'intelligence').status).toBe('allowed');
  });
});
