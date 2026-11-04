import type * as ssoApi from '@/api/sso';

import {
  inputIsMissing,
  type PlatformChoice,
  type PresetInputs,
  parsedOauth2Inputs,
  parsedOidcInputs,
  presetFor,
  presetInitialInputs,
  templatePresetFor,
  visibleInputs,
} from './presets';

export interface ProviderDraft {
  name: string;
  kind: ssoApi.SsoProviderKind;
  platform: ssoApi.SsoPlatform;
  /**
   * What the admin typed into the preset's own fields; the endpoints follow from it.
   * Null when there is no such preset, or a saved provider's endpoints no longer
   * match it: the endpoints are then edited directly.
   */
  presetInputs: PresetInputs | null;
  oidc: ssoApi.OidcProviderConfig;
  oauth2: ssoApi.Oauth2ProviderConfig;
  saml: ssoApi.SamlProviderConfig;
  ldap: ssoApi.LdapProviderConfig;
  defaultRoleId: string;
  roleMappings: RoleMappingDraft[];
  allowedGroups: string[];
}

export interface RoleMappingDraft {
  group: string;
  roleId: string;
}

export interface SsoCallbackUrls {
  /** The instance's address; a new SAML provider's SP entity ID. */
  base: string;
  /** OIDC and OAuth 2.0 share one callback. */
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
    base: baseUrl,
    oidc: `${baseUrl}/api/v1/auth/sso/callback`,
    saml: `${baseUrl}/api/v1/auth/sso/saml/callback`,
  };
}

export function applySsoCallbackUrls(
  draft: ProviderDraft,
  urls: SsoCallbackUrls,
): ProviderDraft {
  if (
    draft.oidc.redirectUri === urls.oidc &&
    draft.oauth2.redirectUri === urls.oidc &&
    draft.saml.assertionConsumerUrl === urls.saml
  ) {
    return draft;
  }
  return {
    ...draft,
    oidc: { ...draft.oidc, redirectUri: urls.oidc },
    oauth2: { ...draft.oauth2, redirectUri: urls.oidc },
    saml: { ...draft.saml, assertionConsumerUrl: urls.saml },
  };
}

const ACCESS_DEFAULTS = { groupRoleMapping: {}, allowedGroups: [] };

function emptyOidc(): ssoApi.OidcProviderConfig {
  return {
    issuer: '',
    authorizeUrl: '',
    tokenUrl: '',
    userinfoUrl: '',
    jwksUri: '',
    clientId: '',
    clientSecret: '',
    redirectUri: '',
    scopes: ['openid', 'email', 'profile'],
    fieldMapping: {
      subject: 'sub',
      email: 'email',
      displayName: 'name',
      groups: 'groups',
    },
    ...ACCESS_DEFAULTS,
  };
}

function emptyOauth2(): ssoApi.Oauth2ProviderConfig {
  return {
    authorizeUrl: '',
    tokenUrl: '',
    userinfoUrl: '',
    clientId: '',
    clientSecret: '',
    redirectUri: '',
    scopes: [],
    fieldMapping: {
      subject: 'id',
      email: 'email',
      displayName: 'name',
      groups: 'groups',
    },
    ...ACCESS_DEFAULTS,
  };
}

function emptySaml(): ssoApi.SamlProviderConfig {
  return {
    spEntityId: '',
    idpEntityId: '',
    idpSsoUrl: '',
    idpX509Cert: '',
    assertionConsumerUrl: '',
    fieldMapping: {
      subject: 'NameID',
      email: 'email',
      displayName: 'name',
      groups: 'groups',
    },
    ...ACCESS_DEFAULTS,
  };
}

function emptyLdap(): ssoApi.LdapProviderConfig {
  return {
    url: 'ldaps://',
    startTls: false,
    bindDn: '',
    bindPassword: '',
    baseDn: '',
    userFilter: '(&(objectClass=person)(|(mail={username})(uid={username})))',
    fieldMapping: {
      subject: 'dn',
      email: 'mail',
      displayName: 'displayName',
      groups: 'memberOf',
    },
    ...ACCESS_DEFAULTS,
  };
}

export function emptyDraft(): ProviderDraft {
  return {
    name: '',
    kind: 'oidc',
    platform: 'custom',
    presetInputs: null,
    oidc: emptyOidc(),
    oauth2: emptyOauth2(),
    saml: emptySaml(),
    ldap: emptyLdap(),
    defaultRoleId: '',
    roleMappings: [],
    allowedGroups: [],
  };
}

/** Copies the preset's endpoints into the draft from the current inputs. */
function withPresetEndpoints(draft: ProviderDraft): ProviderDraft {
  const preset = templatePresetFor(draft.platform, draft.kind);
  if (!preset || !draft.presetInputs) return draft;
  if (preset.kind === 'oidc') {
    const endpoints = preset.endpoints(draft.presetInputs);
    return {
      ...draft,
      oidc: {
        ...draft.oidc,
        issuer: endpoints?.issuer ?? '',
        authorizeUrl: endpoints?.authorize_url ?? '',
        tokenUrl: endpoints?.token_url ?? '',
        userinfoUrl: endpoints?.userinfo_url ?? '',
        jwksUri: endpoints?.jwks_uri ?? '',
      },
    };
  }
  const endpoints = preset.endpoints(draft.presetInputs);
  return {
    ...draft,
    oauth2: {
      ...draft.oauth2,
      authorizeUrl: endpoints?.authorize_url ?? '',
      tokenUrl: endpoints?.token_url ?? '',
      userinfoUrl: endpoints?.userinfo_url ?? '',
    },
  };
}

/**
 * A new draft for what was picked: the preset's endpoints, scopes, field mapping and
 * directory defaults, named after the platform.
 */
export function draftForChoice(
  choice: PlatformChoice,
  name: string,
  urls: SsoCallbackUrls,
): ProviderDraft {
  const empty = emptyDraft();
  const draft: ProviderDraft = {
    ...empty,
    name,
    platform: choice.platform,
    kind: choice.kind,
    saml: { ...empty.saml, spEntityId: urls.base },
  };
  const preset = presetFor(choice.platform, choice.kind);
  if (!preset) return applySsoCallbackUrls(draft, urls);

  const field_mapping = { ...preset.fieldMapping };
  switch (preset.kind) {
    case 'oidc':
      draft.oidc = { ...draft.oidc, scopes: [...preset.scopes], fieldMapping: field_mapping };
      draft.presetInputs = presetInitialInputs(preset);
      break;
    case 'oauth2':
      draft.oauth2 = { ...draft.oauth2, scopes: [...preset.scopes], fieldMapping: field_mapping };
      draft.presetInputs = presetInitialInputs(preset);
      break;
    case 'saml':
      draft.saml = { ...draft.saml, fieldMapping: field_mapping };
      break;
    case 'ldap':
      draft.ldap = { ...draft.ldap, userFilter: preset.userFilter, fieldMapping: field_mapping };
      break;
  }
  return applySsoCallbackUrls(withPresetEndpoints(draft), urls);
}

/**
 * Another protocol of the same platform. A preset starts over from its defaults; the
 * name, role bindings and allowed groups carry over.
 */
export function switchKind(
  draft: ProviderDraft,
  kind: ssoApi.SsoProviderKind,
  urls: SsoCallbackUrls,
): ProviderDraft {
  if (draft.platform === 'custom') return { ...draft, kind };
  const next = draftForChoice({ platform: draft.platform, kind }, draft.name, urls);
  return {
    ...next,
    defaultRoleId: draft.defaultRoleId,
    roleMappings: draft.roleMappings,
    allowedGroups: draft.allowedGroups,
  };
}

export function setPresetInputs(draft: ProviderDraft, inputs: PresetInputs): ProviderDraft {
  return withPresetEndpoints({ ...draft, presetInputs: inputs });
}

function selectedConfig(draft: Pick<ProviderDraft, 'kind' | 'oidc' | 'oauth2' | 'saml' | 'ldap'>) {
  switch (draft.kind) {
    case 'oidc':
      return draft.oidc;
    case 'oauth2':
      return draft.oauth2;
    case 'saml':
      return draft.saml;
    case 'ldap':
      return draft.ldap;
  }
}

export function draftFromProvider(provider: ssoApi.SsoProvider): ProviderDraft {
  const empty = emptyDraft();
  const oidc = { ...empty.oidc, ...(provider.oidc ?? {}), clientSecret: '' };
  const oauth2 = { ...empty.oauth2, ...(provider.oauth2 ?? {}), clientSecret: '' };
  const preset = templatePresetFor(provider.platform, provider.kind);
  const presetInputs =
    preset?.kind === 'oidc' && provider.oidc
      ? parsedOidcInputs(preset, provider.oidc)
      : preset?.kind === 'oauth2' && provider.oauth2
        ? parsedOauth2Inputs(preset, provider.oauth2)
        : null;
  const draft: ProviderDraft = {
    name: provider.name,
    kind: provider.kind,
    platform: provider.platform,
    presetInputs: presetInputs && preset ? { ...presetInitialInputs(preset), ...presetInputs } : null,
    oidc: { ...oidc, userinfoUrl: oidc.userinfoUrl ?? '', jwksUri: oidc.jwksUri ?? '' },
    oauth2,
    saml: { ...empty.saml, ...(provider.saml ?? {}) },
    ldap: { ...empty.ldap, ...(provider.ldap ?? {}), bindPassword: '' },
    defaultRoleId: '',
    roleMappings: [],
    allowedGroups: [],
  };
  const config = selectedConfig(draft);
  return {
    ...draft,
    defaultRoleId: config.defaultRoleId ?? '',
    roleMappings: Object.entries(config.groupRoleMapping).map(([group, roleId]) => ({
      group,
      roleId,
    })),
    allowedGroups: [...config.allowedGroups],
  };
}

export function draftToInput(draft: ProviderDraft): ssoApi.SsoProviderInput {
  const common = { name: draft.name, platform: draft.platform };
  const access = {
    groupRoleMapping: Object.fromEntries(
      draft.roleMappings.map(({ group, roleId }) => [
        group.trim(),
        roleId,
      ]),
    ),
    ...(draft.defaultRoleId
      ? { defaultRoleId: draft.defaultRoleId }
      : {}),
    allowedGroups: draft.allowedGroups.map((group) => group.trim()).filter(Boolean),
  };
  switch (draft.kind) {
    case 'oidc':
      return { ...common, kind: 'oidc', oidc: { ...draft.oidc, ...access } };
    case 'oauth2':
      return { ...common, kind: 'oauth2', oauth2: { ...draft.oauth2, ...access } };
    case 'saml':
      return { ...common, kind: 'saml', saml: { ...draft.saml, ...access } };
    case 'ldap':
      return { ...common, kind: 'ldap', ldap: { ...draft.ldap, ...access } };
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
  const preset = templatePresetFor(draft.platform, draft.kind);
  if (
    preset &&
    draft.presetInputs &&
    visibleInputs(preset, draft.presetInputs).some((input) =>
      inputIsMissing(input, draft.presetInputs?.[input.key]),
    )
  ) {
    return true;
  }
  if (draft.kind === 'oidc') {
    return fieldMappingIsInvalid(draft.oidc.fieldMapping, 'oidc') || [
      draft.oidc.issuer,
      draft.oidc.authorizeUrl,
      draft.oidc.tokenUrl,
      draft.oidc.clientId,
      draft.oidc.redirectUri,
    ].some((value) => !value.trim());
  }
  if (draft.kind === 'oauth2') {
    const secretMissing = !draft.oauth2.clientSecret && !draft.oauth2.hasClientSecret;
    return secretMissing || fieldMappingIsInvalid(draft.oauth2.fieldMapping, 'oauth2') || [
      draft.oauth2.authorizeUrl,
      draft.oauth2.tokenUrl,
      draft.oauth2.userinfoUrl,
      draft.oauth2.clientId,
      draft.oauth2.redirectUri,
    ].some((value) => !value.trim());
  }
  if (draft.kind === 'saml') {
    return fieldMappingIsInvalid(draft.saml.fieldMapping, 'saml') || [
      draft.saml.spEntityId,
      draft.saml.idpEntityId,
      draft.saml.idpSsoUrl,
      draft.saml.idpX509Cert,
      draft.saml.assertionConsumerUrl,
    ].some((value) => !value.trim());
  }

  const ldap = draft.ldap;
  const hasBindDn = Boolean(ldap.bindDn.trim());
  const hasTypedBindPassword = Boolean(ldap.bindPassword);
  const bindPasswordAvailable = Boolean(
    hasTypedBindPassword || ldap.hasBindPassword,
  );
  return (
    !ldapUrlIsSecure(ldap.url, ldap.startTls) ||
    !ldap.baseDn.trim() ||
    !ldap.userFilter.includes('{username}') ||
    fieldMappingIsInvalid(ldap.fieldMapping, 'ldap') ||
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
    mapping.displayName,
    mapping.groups,
  ].map((value) => value.trim());
  if (values.some((value) => !value || value.length > 256)) return true;
  // OIDC and OAuth 2.0 read JSON claims: dotted paths, several joined by `|`.
  if (
    (kind === 'oidc' || kind === 'oauth2') &&
    values.some((value) =>
      value
        .split('|')
        .some((path) => path.trim().split('.').some((part) => !part)),
    )
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
