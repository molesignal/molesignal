import { describe, expect, it } from 'vitest';

import {
  applySsoCallbackUrls,
  draftIsInvalid,
  draftToInput,
  emptyDraft,
  resolveSsoCallbackUrls,
} from './model';

describe('SSO provider draft', () => {
  it('derives fixed OIDC and SAML callback URLs from the external origin', () => {
    expect(
      resolveSsoCallbackUrls(
        'https://observe.example.com/',
        'http://localhost:5173',
      ),
    ).toEqual({
      oidc: 'https://observe.example.com/api/v1/auth/sso/callback',
      saml: 'https://observe.example.com/api/v1/auth/sso/saml/callback',
    });
    expect(resolveSsoCallbackUrls('', 'http://localhost:5173')).toEqual({
      oidc: 'http://localhost:5173/api/v1/auth/sso/callback',
      saml: 'http://localhost:5173/api/v1/auth/sso/saml/callback',
    });
  });

  it('applies both callback URLs to the persisted provider configs', () => {
    const draft = applySsoCallbackUrls(emptyDraft(), {
      oidc: 'https://observe.example.com/api/v1/auth/sso/callback',
      saml: 'https://observe.example.com/api/v1/auth/sso/saml/callback',
    });

    expect(draft.oidc.redirect_uri).toContain('/auth/sso/callback');
    expect(draft.saml.assertion_consumer_url).toContain(
      '/auth/sso/saml/callback',
    );
  });

  it('requires encryption and the username placeholder for LDAP', () => {
    const draft = emptyDraft();
    draft.name = 'Directory';
    draft.kind = 'ldap';
    draft.ldap.url = 'ldap://ldap.example.com:389';
    draft.ldap.base_dn = 'dc=example,dc=com';
    draft.ldap.user_filter = '(mail={username})';

    expect(draftIsInvalid(draft)).toBe(true);
    draft.ldap.start_tls = true;
    expect(draftIsInvalid(draft)).toBe(false);
    draft.ldap.user_filter = '(mail=hard-coded@example.com)';
    expect(draftIsInvalid(draft)).toBe(true);
  });

  it('sends only the selected provider config', () => {
    const draft = emptyDraft();
    draft.name = 'Directory';
    draft.kind = 'ldap';
    draft.defaultRoleId = 'role-viewer';
    draft.roleMappings = [
      { group: 'platform-admins', roleId: 'role-admin' },
    ];

    expect(draftToInput(draft)).toEqual({
      name: 'Directory',
      kind: 'ldap',
      ldap: {
        ...draft.ldap,
        default_role_id: 'role-viewer',
        group_role_mapping: {
          'platform-admins': 'role-admin',
        },
      },
    });
  });

  it('rejects invalid field paths and duplicate external groups', () => {
    const draft = emptyDraft();
    draft.name = 'Corporate OIDC';
    draft.kind = 'oidc';
    draft.oidc.issuer = 'https://idp.example.com';
    draft.oidc.authorize_url = 'https://idp.example.com/authorize';
    draft.oidc.token_url = 'https://idp.example.com/token';
    draft.oidc.client_id = 'client';
    draft.oidc.redirect_uri = 'https://app.example.com/callback';
    draft.oidc.field_mapping.groups = 'realm_access..roles';
    expect(draftIsInvalid(draft)).toBe(true);

    draft.oidc.field_mapping.groups = 'realm_access.roles';
    draft.roleMappings = [
      { group: 'admins', roleId: 'role-admin' },
      { group: ' admins ', roleId: 'role-viewer' },
    ];
    expect(draftIsInvalid(draft)).toBe(true);
  });
});
