import type * as ssoApi from '@/api/sso';

import { LDAP_PRESETS } from './ldap';
import { OAUTH2_PRESETS } from './oauth2';
import { OIDC_PRESETS } from './oidc';
import { SAML_PRESETS } from './saml';
import type { SsoPreset, TemplatePreset } from './types';

export * from './types';
export { baseUrl, hostOf, parsedOidcInputs } from './oidc';
export { parsedOauth2Inputs } from './oauth2';

const PRESETS: SsoPreset[] = [
  ...OIDC_PRESETS,
  ...OAUTH2_PRESETS,
  ...SAML_PRESETS,
  ...LDAP_PRESETS,
];

const ALL_KINDS: ssoApi.SsoProviderKind[] = ['oidc', 'oauth2', 'saml', 'ldap'];

export function presetFor(
  platform: ssoApi.SsoPlatform,
  kind: ssoApi.SsoProviderKind,
): SsoPreset | null {
  return PRESETS.find((preset) => preset.platform === platform && preset.kind === kind) ?? null;
}

/** An OIDC or OAuth 2.0 preset, whose inputs decide the endpoints. */
export function templatePresetFor(
  platform: ssoApi.SsoPlatform,
  kind: ssoApi.SsoProviderKind,
): TemplatePreset | null {
  const preset = presetFor(platform, kind);
  return preset?.kind === 'oidc' || preset?.kind === 'oauth2' ? preset : null;
}

/** The protocols a platform signs in with, the usual one first; a generic provider takes any. */
export function platformKinds(platform: ssoApi.SsoPlatform): ssoApi.SsoProviderKind[] {
  if (platform === 'custom') return ALL_KINDS;
  return ALL_KINDS.filter((kind) => presetFor(platform, kind) !== null);
}

export interface PlatformChoice {
  platform: ssoApi.SsoPlatform;
  kind: ssoApi.SsoProviderKind;
}

export type PlatformGroupId = 'popular' | 'directory' | 'generic';

/**
 * The picker's groups. A Chinese interface lists the domestic platform first, as the
 * notify connector picker does; generic protocols come last in either language.
 */
export function platformGroups(
  language: string,
): Array<{ id: PlatformGroupId; choices: PlatformChoice[] }> {
  const international: ssoApi.SsoPlatform[] = [
    'google',
    'microsoft',
    'github',
    'gitlab',
    'okta',
    'auth0',
    'keycloak',
    'authentik',
  ];
  const popular = language.toLowerCase().startsWith('zh')
    ? ['feishu' as const, ...international]
    : [...international, 'feishu' as const];
  const directory: ssoApi.SsoPlatform[] = [
    'active_directory',
    'adfs',
    'jumpcloud',
    'openldap',
    'freeipa',
  ];
  const first = (platform: ssoApi.SsoPlatform): PlatformChoice => ({
    platform,
    kind: platformKinds(platform)[0] ?? 'oidc',
  });
  return [
    { id: 'popular', choices: popular.map(first) },
    { id: 'directory', choices: directory.map(first) },
    { id: 'generic', choices: ALL_KINDS.map((kind) => ({ platform: 'custom', kind })) },
  ];
}
