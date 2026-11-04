import { randomUUID } from 'node:crypto';

import type { Express } from 'express';

/** The directory takes this password for any account; every other one is refused. */
export const MOCK_DIRECTORY_PASSWORD = 'directory-pass-1';

/** The saved client ID of the mock's `okta`: another one changes its settings. */
export const MOCK_OKTA_CLIENT_ID = '0oa1molesignal';

/** Where a test sign-in goes. A spec acts out the trip: nothing is listening there. */
export const MOCK_IDP_ORIGIN = 'https://idp.mock.test';

/** The same moment as the fixture's frozen clock. */
const NOW_MICROS = Date.parse('2026-05-23T10:00:00.000Z') * 1000;

/** How a spec's trip to the identity provider ends. */
export type MockSsoTestOutcome =
  | 'passed'
  | 'token_exchange'
  | 'another_account'
  | 'new_account'
  | 'link';

interface MockSsoTest {
  id: string;
  providerId: string;
  providerName: string;
  kind: string;
  platform: string;
  status: 'pending' | 'passed' | 'failed' | 'expired';
  current: boolean;
  startedBy: string;
  startedByYou: boolean;
  startedAt: number;
  finishedAt?: number;
  identity?: object;
  mapping?: object;
  failure?: { code: string; detail?: string };
  notes?: string[];
}

const PROVIDERS: Record<string, { name: string; kind: string; platform: string }> = {
  okta: { name: 'Okta', kind: 'oidc', platform: 'okta' },
  entra: { name: 'Entra ID', kind: 'saml', platform: 'microsoft' },
  directory: { name: 'Directory', kind: 'ldap', platform: 'openldap' },
};

const ADMIN_ROLE = { id: 'role-admin', name: 'Admin' };
const VIEWER_ROLE = { id: 'role-viewer', name: 'Viewer' };

const IDENTITY = {
  email: 'dev@molesignal.local',
  displayName: 'Dev User',
  subject: '00u1molesignal',
  groups: ['dev', 'sre'],
  groupsTotal: 2,
  emailVerified: true,
};

const BOUND_TO_THE_TESTER = {
  account: 'bound',
  accountEmail: 'dev@molesignal.local',
  accountName: 'Dev User',
  isTester: true,
  member: true,
  currentRoles: [ADMIN_ROLE],
  roles: [ADMIN_ROLE],
  roleSource: 'group_mapping',
  matchedGroups: ['sre'],
};

function results(outcome: MockSsoTestOutcome): Partial<MockSsoTest> {
  switch (outcome) {
    case 'passed':
      return { status: 'passed', identity: IDENTITY, mapping: BOUND_TO_THE_TESTER };
    case 'token_exchange':
      return {
        status: 'failed',
        failure: {
          code: 'token_exchange',
          detail: 'idp token endpoint 401 Unauthorized: invalid_client',
        },
      };
    case 'another_account':
      return {
        status: 'passed',
        identity: { ...IDENTITY, email: 'someone@example.com', groups: ['contractors'], groupsTotal: 1 },
        mapping: {
          ...BOUND_TO_THE_TESTER,
          accountEmail: 'someone@example.com',
          accountName: 'Someone',
          isTester: false,
          roles: [VIEWER_ROLE],
          roleSource: 'default_role',
          matchedGroups: [],
        },
        notes: ['groups_unmapped', 'another_account', 'roles_change'],
      };
    case 'new_account':
      return {
        status: 'passed',
        identity: { ...IDENTITY, email: 'new@example.com', groups: [], groupsTotal: 0 },
        mapping: {
          account: 'create',
          accountEmail: 'new@example.com',
          isTester: false,
          member: false,
          currentRoles: [],
          roles: [VIEWER_ROLE],
          roleSource: 'signup',
          matchedGroups: [],
        },
        notes: ['another_account'],
      };
    case 'link':
      return {
        status: 'passed',
        identity: IDENTITY,
        mapping: { ...BOUND_TO_THE_TESTER, account: 'link' },
      };
  }
}

/**
 * Test sign-ins of the mock's providers. `okta` starts out with a pass on its current
 * settings. A spec ends a trip with `POST /__mock/sso-tests/:id/finish`, and the
 * settings change with `PUT /api/v1/sso/providers/:id`, as on the real server.
 */
export function registerSsoTestRoutes(app: Express) {
  const tests = new Map<string, MockSsoTest>();

  const describeTest = (providerId: string): MockSsoTest => {
    const provider = PROVIDERS[providerId] ?? { name: providerId, kind: 'oidc', platform: 'custom' };

    return {
      id: randomUUID(),
      providerId: providerId,
      providerName: provider.name,
      kind: provider.kind,
      platform: provider.platform,
      status: 'pending',
      current: true,
      startedBy: 'Dev User',
      startedByYou: true,
      startedAt: NOW_MICROS,
    };
  };

  const seeded: MockSsoTest = {
    ...describeTest('okta'),
    id: 'sso-test-okta',
    ...results('passed'),
    finishedAt: NOW_MICROS,
  };
  tests.set(seeded.id, seeded);

  app.post('/api/v1/sso/providers/:id/tests/ldap', (req, res) => {
    const test = describeTest(req.params.id);
    const right = req.body?.password === MOCK_DIRECTORY_PASSWORD;

    Object.assign(
      test,
      right
        ? {
            status: 'passed',
            identity: { ...IDENTITY, subject: 'uid=dev,ou=people,dc=example,dc=com', groups: ['cn=sre'], groupsTotal: 1 },
            mapping: { ...BOUND_TO_THE_TESTER, matchedGroups: ['cn=sre'] },
            notes: ['second_factor'],
          }
        : {
            status: 'failed',
            failure: {
              code: 'ldap_rejected',
              detail: 'LDAP operation result: rc=49 (invalidCredentials)',
            },
          },
      { finishedAt: NOW_MICROS },
    );

    tests.set(test.id, test);
    res.json(test);
  });

  app.post('/api/v1/sso/providers/:id/tests', (req, res) => {
    const test = describeTest(req.params.id);
    tests.set(test.id, test);
    res.json({
      id: test.id,
      authorizationUrl: `${MOCK_IDP_ORIGIN}/authorize?test=${test.id}&state=opaque-state`,
    });
  });

  app.get('/api/v1/sso/tests/:id', (req, res) => {
    const test = tests.get(req.params.id);
    if (!test) {
      res.status(404).json({ error: 'not_found', message: 'not found: sso sign-in test' });
      return;
    }
    res.json(test);
  });

  app.post('/__mock/sso-tests/:id/finish', (req, res) => {
    const test = tests.get(req.params.id);
    if (!test) {
      res.status(404).end();
      return;
    }
    Object.assign(test, results(req.body.outcome as MockSsoTestOutcome), {
      finishedAt: NOW_MICROS,
    });
    res.json(test);
  });

  app.put('/api/v1/sso/providers/:id', (req, res) => {
    if (req.body?.oidc?.clientId !== MOCK_OKTA_CLIENT_ID) {
      for (const test of tests.values()) {
        if (test.providerId === req.params.id) test.current = false;
      }
    }
    res.json({ id: req.params.id, ...req.body });
  });

  return {
    /** How a provider's current settings stand: the latest result on them, or none. */
    standing(providerId: string) {
      const finished = [...tests.values()]
        .filter((test) => test.providerId === providerId && test.finishedAt !== undefined)
        .reverse();
      const latest = finished.find((test) => test.current) ?? finished[0];

      if (!latest) return { status: 'untested' };

      return {
        status: latest.current ? latest.status : 'stale',
        testId: latest.id,
        finishedAt: latest.finishedAt,
      };
    },
  };
}
