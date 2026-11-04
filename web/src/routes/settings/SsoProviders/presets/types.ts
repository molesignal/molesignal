import type * as ssoApi from '@/api/sso';

/** What an admin typed into a preset's own fields, by input key. */
export type PresetInputs = Record<string, string>;

export interface PresetInput {
  key: string;
  /** Key under `sso_providers.presets.inputs`. */
  label: string;
  placeholder?: string;
  /** Key under `sso_providers.presets.inputs`; a line under the field. */
  hint?: string;
  /** A select instead of a text field; labels are keys under `sso_providers.presets.inputs`. */
  options?: Array<{ value: string; label: string }>;
  initial?: string;
  /** May stay empty. */
  optional?: boolean;
  /** Shown, and required, only while another input has this value. */
  shownWhen?: { key: string; value: string };
  /** The value must match this to be usable. */
  pattern?: RegExp;
}

export interface OidcEndpoints {
  issuer: string;
  authorize_url: string;
  token_url: string;
  userinfo_url: string;
  jwks_uri: string;
}

export interface Oauth2Endpoints {
  authorize_url: string;
  token_url: string;
  userinfo_url: string;
}

interface PresetBase {
  platform: Exclude<ssoApi.SsoPlatform, 'custom'>;
  /** Key under `sso_providers.presets.hints`: where MoleSignal is registered at the platform. */
  hint: string;
  fieldMapping: ssoApi.SsoFieldMapping;
  /** False when the platform sends no groups: everyone gets the default role. */
  groups: boolean;
}

/** OIDC and OAuth 2.0 platforms: a few inputs decide every endpoint. */
interface EndpointPreset<Endpoints> extends PresetBase {
  inputs: PresetInput[];
  scopes: string[];
  /** Null while an input the endpoints need is empty or malformed. */
  endpoints(inputs: PresetInputs): Endpoints | null;
}

export interface OidcPreset extends EndpointPreset<OidcEndpoints> {
  kind: 'oidc';
  /** The inputs a saved configuration was made from, or null when it was edited by hand. */
  parse(config: ssoApi.OidcProviderConfig): PresetInputs | null;
}

export interface Oauth2Preset extends EndpointPreset<Oauth2Endpoints> {
  kind: 'oauth2';
  parse(config: ssoApi.Oauth2ProviderConfig): PresetInputs | null;
  /** The platform reads a fixed set of fields, so the field mapping is not offered. */
  fixedMapping?: boolean;
}

/** SAML platforms: the IdP side comes from its metadata; the preset knows the attribute names. */
export interface SamlPreset extends PresetBase {
  kind: 'saml';
  /** Key under `sso_providers.presets.metadata`: where the metadata is found. */
  metadataPlaceholder: string;
}

/** Directory servers: the preset knows the user filter and attribute names. */
export interface LdapPreset extends PresetBase {
  kind: 'ldap';
  userFilter: string;
  placeholders: { url: string; baseDn: string; bindDn: string };
}

export type TemplatePreset = OidcPreset | Oauth2Preset;
export type SsoPreset = OidcPreset | Oauth2Preset | SamlPreset | LdapPreset;

/** The value of each input, with the initial values filling the gaps. */
export function presetInitialInputs(preset: TemplatePreset): PresetInputs {
  return Object.fromEntries(preset.inputs.map((input) => [input.key, input.initial ?? '']));
}

/** The inputs shown for the current values. */
export function visibleInputs(preset: TemplatePreset, inputs: PresetInputs): PresetInput[] {
  return preset.inputs.filter(
    (input) => !input.shownWhen || inputs[input.shownWhen.key] === input.shownWhen.value,
  );
}

export function inputIsMissing(input: PresetInput, value: string | undefined): boolean {
  const trimmed = value?.trim() ?? '';
  if (!trimmed) return !input.optional;
  return Boolean(input.pattern && !input.pattern.test(trimmed));
}
