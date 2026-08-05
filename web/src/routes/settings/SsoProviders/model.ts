import type * as ssoApi from '@/api/sso';

export interface ProviderDraft {
  name: string;
  kind: ssoApi.SsoProviderKind;
  oidc: ssoApi.OidcProviderConfig;
  saml: ssoApi.SamlProviderConfig;
  ldap: ssoApi.LdapProviderConfig;
  defaultRoleId: string;
  roleMappings: RoleMappingDraft[];
}

export interface RoleMappingDraft {
  group: string;
  roleId: string;
}

export interface SsoCallbackUrls {
  oidc: string;
  saml: string;
}

export function resolveSsoCallbackUrls(
  externalUrl: string | undefined,
  browserOrigin: string,
): SsoCallbackUrls {
  const baseUrl = (externalUrl?.trim() || browserOrigin.trim()).replace(
    /\/+$/,
    '',
  );
  return {
    oidc: `${baseUrl}/api/v1/auth/sso/callback`,
    saml: `${baseUrl}/api/v1/auth/sso/saml/callback`,
  };
}

export function applySsoCallbackUrls(
  draft: ProviderDraft,
  urls: SsoCallbackUrls,
): ProviderDraft {
  if (
    draft.oidc.redirect_uri === urls.oidc &&
    draft.saml.assertion_consumer_url === urls.saml
  ) {
    return draft;
  }
  return {
    ...draft,
    oidc: { ...draft.oidc, redirect_uri: urls.oidc },
    saml: { ...draft.saml, assertion_consumer_url: urls.saml },
  };
}

function emptyOidc(): ssoApi.OidcProviderConfig {
  return {
    issuer: '',
    authorize_url: '',
    token_url: '',
    userinfo_url: '',
    discovery_url: '',
    jwks_uri: '',
    client_id: '',
    client_secret: '',
    redirect_uri: '',
    scopes: ['openid', 'email', 'profile'],
    field_mapping: {
      subject: 'sub',
      email: 'email',
      display_name: 'name',
      groups: 'groups',
    },
    group_role_mapping: {},
  };
}

function emptySaml(): ssoApi.SamlProviderConfig {
  return {
    sp_entity_id: '',
    idp_entity_id: '',
    idp_sso_url: '',
    idp_x509_cert: '',
    assertion_consumer_url: '',
    field_mapping: {
      subject: 'NameID',
      email: 'email',
      display_name: 'name',
      groups: 'groups',
    },
    group_role_mapping: {},
  };
}

function emptyLdap(): ssoApi.LdapProviderConfig {
  return {
    url: 'ldaps://',
    start_tls: false,
    bind_dn: '',
    bind_password: '',
    base_dn: '',
    user_filter: '(&(objectClass=person)(|(mail={username})(uid={username})))',
    field_mapping: {
      subject: 'dn',
      email: 'mail',
      display_name: 'displayName',
      groups: 'memberOf',
    },
    group_role_mapping: {},
  };
}

export function emptyDraft(): ProviderDraft {
  return {
    name: '',
    kind: 'oidc',
    oidc: emptyOidc(),
    saml: emptySaml(),
    ldap: emptyLdap(),
    defaultRoleId: '',
    roleMappings: [],
  };
}

export function draftFromProvider(provider: ssoApi.SsoProvider): ProviderDraft {
  const selectedConfig =
    provider.kind === 'oidc'
      ? provider.oidc
      : provider.kind === 'saml'
        ? provider.saml
        : provider.ldap;
  return {
    name: provider.name,
    kind: provider.kind,
    oidc: {
      ...emptyOidc(),
      ...(provider.oidc ?? {}),
      client_secret: '',
    },
    saml: {
      ...emptySaml(),
      ...(provider.saml ?? {}),
    },
    ldap: {
      ...emptyLdap(),
      ...(provider.ldap ?? {}),
      bind_password: '',
    },
    defaultRoleId: selectedConfig?.default_role_id ?? '',
    roleMappings: Object.entries(
      selectedConfig?.group_role_mapping ?? {},
    ).map(([group, roleId]) => ({ group, roleId })),
  };
}

export function draftToInput(draft: ProviderDraft): ssoApi.SsoProviderInput {
  const common = { name: draft.name, kind: draft.kind };
  const access = {
    group_role_mapping: Object.fromEntries(
      draft.roleMappings.map(({ group, roleId }) => [
        group.trim(),
        roleId,
      ]),
    ),
    ...(draft.defaultRoleId
      ? { default_role_id: draft.defaultRoleId }
      : {}),
  };
  switch (draft.kind) {
    case 'oidc':
      return {
        ...common,
        kind: 'oidc',
        oidc: { ...draft.oidc, ...access },
      };
    case 'saml':
      return {
        ...common,
        kind: 'saml',
        saml: { ...draft.saml, ...access },
      };
    case 'ldap':
      return {
        ...common,
        kind: 'ldap',
        ldap: { ...draft.ldap, ...access },
      };
  }
}

export function draftIsInvalid(draft: ProviderDraft): boolean {
  if (!draft.name.trim()) return true;
  const normalizedGroups = draft.roleMappings.map(({ group }) =>
    group.trim(),
  );
  if (
    draft.roleMappings.some(
      ({ group, roleId }) => !group.trim() || !roleId,
    ) ||
    new Set(normalizedGroups).size !== normalizedGroups.length
  ) {
    return true;
  }
  if (draft.kind === 'oidc') {
    return fieldMappingIsInvalid(draft.oidc.field_mapping, 'oidc') || [
      draft.oidc.issuer,
      draft.oidc.authorize_url,
      draft.oidc.token_url,
      draft.oidc.client_id,
      draft.oidc.redirect_uri,
    ].some((value) => !value.trim());
  }
  if (draft.kind === 'saml') {
    return fieldMappingIsInvalid(draft.saml.field_mapping, 'saml') || [
      draft.saml.sp_entity_id,
      draft.saml.idp_entity_id,
      draft.saml.idp_sso_url,
      draft.saml.idp_x509_cert,
      draft.saml.assertion_consumer_url,
    ].some((value) => !value.trim());
  }

  const ldap = draft.ldap;
  const hasBindDn = Boolean(ldap.bind_dn.trim());
  const hasTypedBindPassword = Boolean(ldap.bind_password);
  const bindPasswordAvailable = Boolean(
    hasTypedBindPassword || ldap.has_bind_password,
  );
  return (
    !ldapUrlIsSecure(ldap.url, ldap.start_tls) ||
    !ldap.base_dn.trim() ||
    !ldap.user_filter.includes('{username}') ||
    fieldMappingIsInvalid(ldap.field_mapping, 'ldap') ||
    (hasBindDn && !bindPasswordAvailable) ||
    (!hasBindDn && hasTypedBindPassword)
  );
}

function fieldMappingIsInvalid(
  mapping: ssoApi.SsoFieldMapping,
  kind: ssoApi.SsoProviderKind,
): boolean {
  const values = [
    mapping.subject,
    mapping.email,
    mapping.display_name,
    mapping.groups,
  ].map((value) => value.trim());
  if (values.some((value) => !value || value.length > 256)) return true;
  if (
    kind === 'oidc' &&
    values.some((value) => value.split('.').some((part) => !part))
  ) {
    return true;
  }
  return kind === 'ldap' && values.some((value) => /\s/.test(value));
}

function ldapUrlIsSecure(raw: string, startTls: boolean): boolean {
  try {
    const url = new URL(raw);
    if (!url.hostname || url.username || url.password) return false;
    if (url.protocol === 'ldaps:') return !startTls;
    return url.protocol === 'ldap:' && startTls;
  } catch {
    return false;
  }
}
