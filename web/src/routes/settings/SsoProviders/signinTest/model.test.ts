import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import type * as ssoApi from '@/api/sso';
import enAdmin from '@/i18n/en-us/settings-admin.json';
import zhAdmin from '@/i18n/zh-cn/settings-admin.json';

import {
  enforcementCheck,
  failureCodeOf,
  knownNotes,
  rolesChange,
  singleSignOnProviders,
  standingOf,
  TEST_FAILURE_CODES,
  TEST_NOTE_CODES,
  testResultPath,
} from './model';

function provider(
  name: string,
  overrides: Partial<ssoApi.SsoProvider> = {},
): ssoApi.SsoProvider {
  return {
    id: name,
    orgId: 'org',
    name,
    kind: 'oidc',
    platform: 'custom',
    enabled: true,
    ...overrides,
  };
}

function mapping(overrides: Partial<ssoApi.SsoTestedMapping> = {}): ssoApi.SsoTestedMapping {
  return {
    account: 'bound',
    accountEmail: 'alice@example.com',
    isTester: true,
    member: true,
    currentRoles: [{ id: 'a', name: 'A' }],
    roles: [{ id: 'a', name: 'A' }],
    roleSource: 'unchanged',
    matchedGroups: [],
    ...overrides,
  };
}

describe('the provider standing', () => {
  it('a provider read without a standing counts as not tested', () => {
    expect(standingOf(provider('a')).status).toBe('untested');
    expect(standingOf(provider('a', { test: { status: 'passed' } })).status).toBe('passed');
  });

  it('a result page is addressed by the test', () => {
    expect(testResultPath('t 1')).toBe('/iam/sso/tests/t%201');
  });

  it('the result page has its row in the route catalog, or a real backend denies it', () => {
    const catalog = readFileSync(
      resolve(process.cwd(), '../crates/engines/postgres/src/migrations/20260101000003_iam_route_catalog.sql'),
      'utf8',
    );

    expect(catalog).toContain("('iam.sso.test', '/iam/sso/tests/:id', 'organization', 'all'");
    expect(catalog).toMatch(/'\/iam\/sso\/tests\/:id'.*ARRAY\['org\.settings\.manage'\]/);
  });
});

describe('before single sign-on becomes the only way in', () => {
  it('only enabled providers that are single sign-on count', () => {
    const names = singleSignOnProviders([
      provider('on'),
      provider('off', { enabled: false }),
      provider('dir', { kind: 'ldap' }),
      provider('saml', { kind: 'saml' }),
      provider('oauth', { kind: 'oauth2' }),
    ]).map((row) => row.name);

    expect(names).toEqual(['on', 'saml', 'oauth']);
  });

  it('only a pass on the current settings proves a provider', () => {
    const check = enforcementCheck([
      provider('passed', { test: { status: 'passed' } }),
      provider('failed', { test: { status: 'failed' } }),
      provider('changed', { test: { status: 'stale' } }),
      provider('new'),
    ]);

    expect(check.proven.map((row) => row.name)).toEqual(['passed']);
    expect(check.unproven.map((row) => row.name)).toEqual(['failed', 'changed', 'new']);
  });

  it('no providers prove nothing', () => {
    expect(enforcementCheck([])).toEqual({ proven: [], unproven: [] });
  });
});

describe('what the server reports', () => {
  it('a failure this page has no words for reads as unknown', () => {
    expect(failureCodeOf('token_exchange')).toBe('token_exchange');
    expect(failureCodeOf('from_a_newer_server')).toBe('unknown');
    expect(failureCodeOf(undefined)).toBe('unknown');
  });

  it('notes this page has no words for are left out', () => {
    expect(knownNotes(['roles_change', 'from_a_newer_server', 'no_groups'])).toEqual([
      'no_groups',
      'roles_change',
    ]);
    expect(knownNotes(undefined)).toEqual([]);
  });

  it('every failure and note has its words in both languages', () => {
    const missing: string[] = [];

    for (const [language, admin] of [
      ['en', enAdmin],
      ['zh', zhAdmin],
    ] as const) {
      const failures = admin.sso_providers.test.failures as Record<
        string,
        { title?: string; advice?: string } | undefined
      >;
      const notes = admin.sso_providers.test.notes as Record<string, string | undefined>;

      for (const code of TEST_FAILURE_CODES) {
        if (!failures[code]?.title || !failures[code]?.advice) missing.push(`${language} ${code}`);
      }
      for (const code of TEST_NOTE_CODES) {
        if (!notes[code]) missing.push(`${language} note ${code}`);
      }
    }

    expect(missing).toEqual([]);
  });

  it('the words cover what the server can report and nothing else', () => {
    expect(Object.keys(enAdmin.sso_providers.test.failures).sort()).toEqual(
      [...TEST_FAILURE_CODES].sort(),
    );
    expect(Object.keys(enAdmin.sso_providers.test.notes).sort()).toEqual(
      [...TEST_NOTE_CODES].sort(),
    );
  });
});

describe('roles at sign-in', () => {
  it('a member keeps roles that only come in another order', () => {
    expect(
      rolesChange(
        mapping({
          currentRoles: [
            { id: 'a', name: 'A' },
            { id: 'b', name: 'B' },
          ],
          roles: [
            { id: 'b', name: 'B' },
            { id: 'a', name: 'A' },
          ],
        }),
      ),
    ).toBe(false);
  });

  it('a member whose roles differ is told', () => {
    expect(rolesChange(mapping({ roles: [{ id: 'b', name: 'B' }] }))).toBe(true);
  });

  it('a new member has nothing to lose', () => {
    expect(
      rolesChange(mapping({ member: false, currentRoles: [], roles: [{ id: 'b', name: 'B' }] })),
    ).toBe(false);
  });
});
