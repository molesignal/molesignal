import type * as ssoApi from '@/api/sso';

import type { OidcEndpoints, OidcPreset, PresetInputs } from './types';

/** `https://` added when missing, trailing slashes dropped. Empty stays empty. */
export function baseUrl(value: string | undefined): string {
  const trimmed = value?.trim() ?? '';
  if (!trimmed) return '';
  const withScheme = /^https?:\/\//i.test(trimmed) ? trimmed : `https://${trimmed}`;
  return withScheme.replace(/\/+$/, '');
}

/** The host of a domain typed with or without scheme and path. */
export function hostOf(value: string | undefined): string {
  return (value?.trim() ?? '')
    .replace(/^https?:\/\//i, '')
    .replace(/[/?#].*$/, '')
    .toLowerCase();
}

const GUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

const STANDARD_MAPPING: ssoApi.SsoFieldMapping = {
  subject: 'sub',
  email: 'email',
  displayName: 'name',
  groups: 'groups',
};

const STANDARD_SCOPES = ['openid', 'profile', 'email'];

const MICROSOFT_HOSTS: Record<string, string> = {
  global: 'login.microsoftonline.com',
  china: 'login.partner.microsoftonline.cn',
};

function oidcAt(issuer: string, paths: Omit<OidcEndpoints, 'issuer'>): OidcEndpoints {
  return { issuer, ...paths };
}

export const OIDC_PRESETS: OidcPreset[] = [
  {
    platform: 'google',
    kind: 'oidc',
    inputs: [],
    scopes: STANDARD_SCOPES,
    fieldMapping: STANDARD_MAPPING,
    groups: false,
    hint: 'google',
    endpoints: () =>
      oidcAt('https://accounts.google.com', {
        authorize_url: 'https://accounts.google.com/o/oauth2/v2/auth',
        token_url: 'https://oauth2.googleapis.com/token',
        userinfo_url: '',
        jwks_uri: 'https://www.googleapis.com/oauth2/v3/certs',
      }),
    parse: (config) => (config.issuer === 'https://accounts.google.com' ? {} : null),
  },
  {
    platform: 'microsoft',
    kind: 'oidc',
    inputs: [
      {
        key: 'tenant',
        label: 'tenant_id',
        placeholder: '00000000-0000-0000-0000-000000000000',
        hint: 'tenant_id_hint',
        pattern: GUID,
      },
      {
        key: 'cloud',
        label: 'cloud',
        initial: 'global',
        options: [
          { value: 'global', label: 'cloud_global' },
          { value: 'china', label: 'cloud_china' },
        ],
      },
    ],
    scopes: STANDARD_SCOPES,
    // `oid` is the user in the tenant; `email` is empty for accounts without a mailbox.
    fieldMapping: {
      subject: 'oid | sub',
      email: 'email | preferred_username',
      displayName: 'name',
      groups: 'groups',
    },
    groups: true,
    hint: 'microsoft',
    endpoints: ({ tenant, cloud }) => {
      const id = tenant?.trim().toLowerCase() ?? '';
      if (!GUID.test(id)) return null;
      const base = `https://${MICROSOFT_HOSTS[cloud ?? 'global'] ?? MICROSOFT_HOSTS.global}/${id}`;
      return oidcAt(`${base}/v2.0`, {
        authorize_url: `${base}/oauth2/v2.0/authorize`,
        token_url: `${base}/oauth2/v2.0/token`,
        userinfo_url: '',
        jwks_uri: `${base}/discovery/v2.0/keys`,
      });
    },
    parse: (config) => {
      const match = /^https:\/\/([^/]+)\/([^/]+)\/v2\.0$/.exec(config.issuer);
      const cloud = Object.entries(MICROSOFT_HOSTS).find(([, host]) => host === match?.[1])?.[0];
      return match && cloud ? { tenant: match[2] ?? '', cloud } : null;
    },
  },
  {
    platform: 'okta',
    kind: 'oidc',
    inputs: [
      { key: 'domain', label: 'okta_domain', placeholder: 'dev-123456.okta.com' },
      {
        key: 'server',
        label: 'authorization_server',
        initial: 'default',
        hint: 'authorization_server_hint',
        optional: true,
      },
    ],
    scopes: STANDARD_SCOPES,
    fieldMapping: STANDARD_MAPPING,
    groups: true,
    hint: 'okta',
    endpoints: ({ domain, server }) => {
      const host = hostOf(domain);
      if (!host) return null;
      const name = server?.trim() ?? '';
      const issuer = name ? `https://${host}/oauth2/${name}` : `https://${host}`;
      const api = name ? `${issuer}/v1` : `https://${host}/oauth2/v1`;
      return oidcAt(issuer, {
        authorize_url: `${api}/authorize`,
        token_url: `${api}/token`,
        userinfo_url: `${api}/userinfo`,
        jwks_uri: `${api}/keys`,
      });
    },
    parse: (config) => {
      const match = /^https:\/\/([^/]+)(?:\/oauth2\/([^/]+))?$/.exec(config.issuer);
      return match ? { domain: match[1] ?? '', server: match[2] ?? '' } : null;
    },
  },
  {
    platform: 'auth0',
    kind: 'oidc',
    inputs: [{ key: 'domain', label: 'auth0_domain', placeholder: 'your-tenant.us.auth0.com' }],
    scopes: STANDARD_SCOPES,
    fieldMapping: STANDARD_MAPPING,
    groups: true,
    hint: 'auth0',
    endpoints: ({ domain }) => {
      const host = hostOf(domain);
      if (!host) return null;
      // Auth0 publishes its issuer with the trailing slash.
      return oidcAt(`https://${host}/`, {
        authorize_url: `https://${host}/authorize`,
        token_url: `https://${host}/oauth/token`,
        userinfo_url: `https://${host}/userinfo`,
        jwks_uri: `https://${host}/.well-known/jwks.json`,
      });
    },
    parse: (config) => {
      const match = /^https:\/\/([^/]+)\/$/.exec(config.issuer);
      return match ? { domain: match[1] ?? '' } : null;
    },
  },
  {
    platform: 'keycloak',
    kind: 'oidc',
    inputs: [
      { key: 'base_url', label: 'server_url', placeholder: 'https://sso.example.com' },
      { key: 'realm', label: 'realm', placeholder: 'ops' },
    ],
    scopes: STANDARD_SCOPES,
    fieldMapping: STANDARD_MAPPING,
    groups: true,
    hint: 'keycloak',
    endpoints: ({ base_url, realm }) => {
      const base = baseUrl(base_url);
      const name = realm?.trim() ?? '';
      if (!base || !name) return null;
      const issuer = `${base}/realms/${encodeURIComponent(name)}`;
      const protocol = `${issuer}/protocol/openid-connect`;
      return oidcAt(issuer, {
        authorize_url: `${protocol}/auth`,
        token_url: `${protocol}/token`,
        userinfo_url: `${protocol}/userinfo`,
        jwks_uri: `${protocol}/certs`,
      });
    },
    parse: (config) => {
      const match = /^(https?:\/\/.+)\/realms\/([^/]+)$/.exec(config.issuer);
      return match
        ? { base_url: match[1] ?? '', realm: decodeURIComponent(match[2] ?? '') }
        : null;
    },
  },
  {
    platform: 'authentik',
    kind: 'oidc',
    inputs: [
      { key: 'base_url', label: 'server_url', placeholder: 'https://authentik.example.com' },
      { key: 'slug', label: 'application_slug', placeholder: 'molesignal' },
    ],
    scopes: STANDARD_SCOPES,
    fieldMapping: STANDARD_MAPPING,
    groups: true,
    hint: 'authentik',
    endpoints: ({ base_url, slug }) => {
      const base = baseUrl(base_url);
      const name = slug?.trim() ?? '';
      if (!base || !name) return null;
      return oidcAt(`${base}/application/o/${name}/`, {
        authorize_url: `${base}/application/o/authorize/`,
        token_url: `${base}/application/o/token/`,
        userinfo_url: `${base}/application/o/userinfo/`,
        jwks_uri: `${base}/application/o/${name}/jwks/`,
      });
    },
    parse: (config) => {
      const match = /^(https?:\/\/.+)\/application\/o\/([^/]+)\/$/.exec(config.issuer);
      return match ? { base_url: match[1] ?? '', slug: match[2] ?? '' } : null;
    },
  },
  {
    platform: 'gitlab',
    kind: 'oidc',
    inputs: [{ key: 'base_url', label: 'instance_url', initial: 'https://gitlab.com' }],
    scopes: STANDARD_SCOPES,
    // GitLab sends `groups` (every group, inherited ones too) from its UserInfo endpoint.
    fieldMapping: STANDARD_MAPPING,
    groups: true,
    hint: 'gitlab',
    endpoints: ({ base_url }) => {
      const base = baseUrl(base_url);
      if (!base) return null;
      return oidcAt(base, {
        authorize_url: `${base}/oauth/authorize`,
        token_url: `${base}/oauth/token`,
        userinfo_url: `${base}/oauth/userinfo`,
        jwks_uri: `${base}/oauth/discovery/keys`,
      });
    },
    parse: (config) =>
      config.authorizeUrl === `${config.issuer}/oauth/authorize`
        ? { base_url: config.issuer }
        : null,
  },
];

/** The inputs of a saved configuration whose endpoints match the preset's. */
export function parsedOidcInputs(
  preset: OidcPreset,
  config: ssoApi.OidcProviderConfig,
): PresetInputs | null {
  const inputs = preset.parse(config);
  if (!inputs) return null;
  const endpoints = preset.endpoints(inputs);
  const matches =
    endpoints !== null &&
    endpoints.issuer === config.issuer &&
    endpoints.authorize_url === config.authorizeUrl &&
    endpoints.token_url === config.tokenUrl &&
    endpoints.userinfo_url === (config.userinfoUrl ?? '') &&
    endpoints.jwks_uri === (config.jwksUri ?? '');
  return matches ? inputs : null;
}
