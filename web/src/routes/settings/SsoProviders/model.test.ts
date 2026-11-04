import { describe, expect, it } from 'vitest';

import type * as ssoApi from '@/api/sso';

import { acceptsPublicAccounts } from './access';
import {
  applySsoCallbackUrls,
  draftForChoice,
  draftFromProvider,
  draftIsInvalid,
  draftToInput,
  emptyDraft,
  resolveSsoCallbackUrls,
  setPresetInputs,
  switchKind,
} from './model';

const URLS = resolveSsoCallbackUrls('https://observe.example.com', 'http://localhost:5173');

describe('SSO provider draft', () => {
  it('derives fixed OIDC and SAML callback URLs from the external origin', () => {
    expect(
      resolveSsoCallbackUrls(
        'https://observe.example.com/',
        'http://localhost:5173',
      ),
    ).toEqual({
      base: 'https://observe.example.com',
      oidc: 'https://observe.example.com/api/v1/auth/sso/callback',
      saml: 'https://observe.example.com/api/v1/auth/sso/saml/callback',
    });
    expect(resolveSsoCallbackUrls('', 'http://localhost:5173')).toEqual({
      base: 'http://localhost:5173',
      oidc: 'http://localhost:5173/api/v1/auth/sso/callback',
      saml: 'http://localhost:5173/api/v1/auth/sso/saml/callback',
    });
  });

  it('applies the callback URLs to every redirect-based config', () => {
    const draft = applySsoCallbackUrls(emptyDraft(), URLS);

    expect(draft.oidc.redirectUri).toBe(URLS.oidc);
    expect(draft.oauth2.redirectUri).toBe(URLS.oidc);
    expect(draft.saml.assertionConsumerUrl).toBe(URLS.saml);
  });

  it('requires encryption and the username placeholder for LDAP', () => {
    const draft = emptyDraft();
    draft.name = 'Directory';
    draft.kind = 'ldap';
    draft.ldap.url = 'ldap://ldap.example.com:389';
    draft.ldap.baseDn = 'dc=example,dc=com';
    draft.ldap.userFilter = '(mail={username})';

    expect(draftIsInvalid(draft)).toBe(true);
    draft.ldap.startTls = true;
    expect(draftIsInvalid(draft)).toBe(false);
    draft.ldap.userFilter = '(mail=hard-coded@example.com)';
    expect(draftIsInvalid(draft)).toBe(true);
  });

  it('sends only the selected provider config, with the platform and allowed groups', () => {
    const draft = emptyDraft();
    draft.name = 'Directory';
    draft.kind = 'ldap';
    draft.defaultRoleId = 'role-viewer';
    draft.roleMappings = [
      { group: 'platform-admins', roleId: 'role-admin' },
    ];
    draft.allowedGroups = [' ops ', ''];

    expect(draftToInput(draft)).toEqual({
      name: 'Directory',
      kind: 'ldap',
      platform: 'custom',
      ldap: {
        ...draft.ldap,
        defaultRoleId: 'role-viewer',
        groupRoleMapping: {
          'platform-admins': 'role-admin',
        },
        allowedGroups: ['ops'],
      },
    });
  });

  it('rejects invalid field paths and duplicate external groups', () => {
    const draft = emptyDraft();
    draft.name = 'Corporate OIDC';
    draft.kind = 'oidc';
    draft.oidc.issuer = 'https://idp.example.com';
    draft.oidc.authorizeUrl = 'https://idp.example.com/authorize';
    draft.oidc.tokenUrl = 'https://idp.example.com/token';
    draft.oidc.clientId = 'client';
    draft.oidc.redirectUri = 'https://app.example.com/callback';
    draft.oidc.fieldMapping.groups = 'realm_access..roles';
    expect(draftIsInvalid(draft)).toBe(true);

    draft.oidc.fieldMapping.groups = 'realm_access.roles | groups';
    expect(draftIsInvalid(draft)).toBe(false);
    draft.oidc.fieldMapping.groups = 'realm_access.roles |';
    expect(draftIsInvalid(draft)).toBe(true);

    draft.oidc.fieldMapping.groups = 'realm_access.roles';
    draft.roleMappings = [
      { group: 'admins', roleId: 'role-admin' },
      { group: ' admins ', roleId: 'role-viewer' },
    ];
    expect(draftIsInvalid(draft)).toBe(true);
  });
});

describe('SSO platform presets', () => {
  it('fills a fixed-endpoint platform without any input', () => {
    const draft = draftForChoice({ platform: 'google', kind: 'oidc' }, 'Google', URLS);

    expect(draft.name).toBe('Google');
    expect(draft.oidc.issuer).toBe('https://accounts.google.com');
    expect(draft.oidc.jwksUri).toBe('https://www.googleapis.com/oauth2/v3/certs');
    expect(draft.oidc.redirectUri).toBe(URLS.oidc);
    expect(draft.presetInputs).toEqual({});
    expect(acceptsPublicAccounts(draft.kind, draft)).toBe(true);

    // Only the client credentials are left to the admin.
    expect(draftIsInvalid(draft)).toBe(true);
    draft.oidc.clientId = 'client.apps.googleusercontent.com';
    expect(draftIsInvalid(draft)).toBe(false);
  });

  it('derives the endpoints from the inputs as they are typed', () => {
    let draft = draftForChoice({ platform: 'keycloak', kind: 'oidc' }, 'Keycloak', URLS);
    expect(draft.oidc.issuer).toBe('');

    draft = setPresetInputs(draft, { base_url: 'sso.example.com/', realm: 'ops' });
    expect(draft.oidc.issuer).toBe('https://sso.example.com/realms/ops');
    expect(draft.oidc.tokenUrl).toBe(
      'https://sso.example.com/realms/ops/protocol/openid-connect/token',
    );

    draft = setPresetInputs(draft, { base_url: 'https://sso.example.com', realm: '' });
    expect(draft.oidc.issuer).toBe('');
    expect(draftIsInvalid({ ...draft, oidc: { ...draft.oidc, clientId: 'c' } })).toBe(true);
  });

  it('checks a Microsoft tenant id and chooses the cloud', () => {
    let draft = draftForChoice({ platform: 'microsoft', kind: 'oidc' }, 'Microsoft', URLS);
    draft = setPresetInputs(draft, { tenant: 'contoso.onmicrosoft.com', cloud: 'global' });
    expect(draft.oidc.issuer).toBe('');

    const tenant = '72F988BF-86F1-41AF-91AB-2D7CD011DB47';
    draft = setPresetInputs(draft, { tenant, cloud: 'china' });
    expect(draft.oidc.issuer).toBe(
      'https://login.partner.microsoftonline.cn/72f988bf-86f1-41af-91ab-2d7cd011db47/v2.0',
    );
    expect(draft.oidc.fieldMapping.email).toBe('email | preferred_username');
    expect(acceptsPublicAccounts(draft.kind, draft)).toBe(false);
  });

  it('signs GitHub in with OAuth 2.0 and treats only github.com as public', () => {
    let draft = draftForChoice({ platform: 'github', kind: 'oauth2' }, 'GitHub', URLS);
    expect(draft.oauth2.userinfoUrl).toBe('https://api.github.com/user');
    expect(draft.oauth2.scopes).toEqual(['read:user', 'user:email', 'read:org']);
    expect(acceptsPublicAccounts(draft.kind, draft)).toBe(true);

    draft = setPresetInputs(draft, { deployment: 'server', host: 'https://github.example.com/' });
    expect(draft.oauth2.userinfoUrl).toBe('https://github.example.com/api/v3/user');
    expect(acceptsPublicAccounts(draft.kind, draft)).toBe(false);

    // OAuth 2.0 apps are confidential: the secret is required.
    draft.oauth2.clientId = 'Iv1.abc';
    expect(draftIsInvalid(draft)).toBe(true);
    draft.oauth2.clientSecret = 'secret';
    expect(draftIsInvalid(draft)).toBe(false);
  });

  it('switches a platform to its other protocol and keeps the bindings', () => {
    const draft = draftForChoice({ platform: 'microsoft', kind: 'oidc' }, 'Entra', URLS);
    draft.defaultRoleId = 'role-viewer';
    draft.allowedGroups = ['sre'];

    const saml = switchKind(draft, 'saml', URLS);
    expect(saml.kind).toBe('saml');
    expect(saml.name).toBe('Entra');
    expect(saml.presetInputs).toBeNull();
    expect(saml.saml.spEntityId).toBe('https://observe.example.com');
    expect(saml.saml.fieldMapping.email).toBe(
      'http://schemas.xmlsoap.org/ws/2005/05/identity/claims/emailaddress',
    );
    expect(saml.defaultRoleId).toBe('role-viewer');
    expect(saml.allowedGroups).toEqual(['sre']);
  });

  it('reads the inputs back from a saved provider, and lets hand-edited endpoints be', () => {
    const created = setPresetInputs(
      draftForChoice({ platform: 'okta', kind: 'oidc' }, 'Okta', URLS),
      { domain: 'dev-1.okta.com', server: 'default' },
    );
    const oidc = { ...created.oidc, allowedGroups: ['admins'] };
    const saved: ssoApi.SsoProvider = {
      id: 'p1',
      orgId: 'o1',
      name: 'Okta',
      kind: 'oidc',
      platform: 'okta',
      enabled: true,
      oidc,
    };

    const reopened = draftFromProvider(saved);
    expect(reopened.presetInputs).toEqual({ domain: 'dev-1.okta.com', server: 'default' });
    expect(reopened.allowedGroups).toEqual(['admins']);

    const edited = draftFromProvider({
      ...saved,
      oidc: { ...oidc, tokenUrl: 'https://proxy.example.com/token' },
    });
    expect(edited.presetInputs).toBeNull();
    expect(edited.oidc.tokenUrl).toBe('https://proxy.example.com/token');
  });

  it('starts directory presets from their filters and attribute names', () => {
    const draft = draftForChoice(
      { platform: 'active_directory', kind: 'ldap' },
      'Active Directory',
      URLS,
    );
    expect(draft.ldap.userFilter).toContain('sAMAccountName={username}');
    expect(draft.ldap.fieldMapping.groups).toBe('memberOf');
    expect(draft.presetInputs).toBeNull();
  });
});
