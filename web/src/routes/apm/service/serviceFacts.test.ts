import { describe, expect, it } from 'vitest';

import type { ServiceProfile, ServiceSummary } from '@/api/apm';

import { deployedAt, entryKindsOf, firstOf, languageName, repositoryHref, repositoryOf, stackOf, versionsInView } from './serviceFacts';

const profile: ServiceProfile = {
  instanceCount: 3,
  hostCount: 2,
  versions: [
    { value: '1.8.2', instances: 2 },
    { value: '1.8.1', instances: 1 },
  ],
  languages: [{ value: 'rust', instances: 3 }],
  runtimes: [],
  frameworks: [{ value: 'Axum', instances: 3 }],
  regions: [
    { value: 'ap-southeast-1', instances: 2 },
    { value: 'eu-west-1', instances: 1 },
  ],
  clusters: [],
  k8sNamespaces: [],
  workloads: [],
  repositories: [],
  commits: [],
  entryKinds: ['messaging', 'http'],
};

const summary = {
  instrumentation: { runtimeLanguage: 'java', recentInstanceCount: 6 },
  versions: ['1.8.1', '1.8.2'],
  currentVersion: '1.8.2',
  currentVersionSince: 500,
  profile,
} as unknown as ServiceSummary;

describe('service facts', () => {
  it('says the stack, the kinds in their order and a ranked list with how many more', () => {
    expect(stackOf(summary)).toBe('Rust / Axum');
    expect(stackOf({ ...summary, profile: null })).toBe('Java');
    expect(stackOf(undefined)).toBeNull();
    expect(languageName('nodejs')).toBe('Node.js');
    expect(entryKindsOf(profile)).toEqual(['http', 'messaging']);
    expect(firstOf(profile.regions)).toEqual({ value: 'ap-southeast-1', more: 1 });
    expect(firstOf([])).toBeNull();
  });

  it('follows the version in view: both versions of a rollout, or the one chosen', () => {
    expect(versionsInView(summary, undefined)).toEqual(['1.8.2', '1.8.1']);
    expect(versionsInView(summary, '1.8.1')).toEqual(['1.8.1']);
    expect(versionsInView({ ...summary, profile: null }, undefined)).toEqual(['1.8.2']);

    const versions = [{ version: '1.8.1', firstSeenAt: 100 }] as never[];
    expect(deployedAt(summary, versions, undefined)).toBe(500);
    expect(deployedAt(summary, versions, '1.8.1')).toBe(100);
  });

  it('takes the repository the telemetry reports before the one set in the catalog', () => {
    expect(repositoryOf(summary, 'https://git.example.com/configured')).toBe('https://git.example.com/configured');
    const reported = { ...summary, profile: { ...profile, repositories: [{ value: 'https://github.com/acme/checkout', instances: 3 }] } };
    expect(repositoryOf(reported, 'https://git.example.com/configured')).toBe('https://github.com/acme/checkout');
  });

  it('opens a git repository at the https page of the same path, without credentials', () => {
    expect(repositoryHref('https://ci:****@git.corp.example:8443/team/checkout.git')).toBe('https://git.corp.example:8443/team/checkout.git');
    expect(repositoryHref('git@github.com:acme/checkout.git')).toBe('https://github.com/acme/checkout');
    expect(repositoryHref('ssh://git@git.corp.example:2222/team/checkout.git')).toBe('https://git.corp.example/team/checkout');
    expect(repositoryHref('git://git.kernel.org/pub/scm/git/git.git')).toBe('https://git.kernel.org/pub/scm/git/git');
    expect(repositoryHref('javascript:alert(1)')).toBeNull();
    expect(repositoryHref('ftp://example.com/repo.git')).toBeNull();
  });
});
