import type * as ssoApi from '@/api/sso';

/**
 * Identity sources anyone can sign up to. Mirrors the server's rule
 * (`api/http/routes/sso/policy.rs`): such a provider is enabled only while the
 * organization restricts email domains or the provider names allowed groups.
 */
const PUBLIC_HOSTS = ['accounts.google.com', 'github.com', 'gitlab.com'];

/** Microsoft's tenant-less paths, and the personal-account tenant. */
const MICROSOFT_OPEN_TENANTS = [
  'common',
  'organizations',
  'consumers',
  '9188040d-6c67-4c5b-b112-36a304b66dad',
];

export function acceptsPublicAccounts(
  kind: ssoApi.SsoProviderKind,
  config: { oidc: ssoApi.OidcProviderConfig; oauth2: ssoApi.Oauth2ProviderConfig },
): boolean {
  const endpoint =
    kind === 'oidc'
      ? config.oidc.issuer
      : kind === 'oauth2'
        ? config.oauth2.authorizeUrl
        : '';
  let url: URL;
  try {
    url = new URL(endpoint.trim());
  } catch {
    return false;
  }
  const host = url.hostname.toLowerCase();
  if (host === 'login.microsoftonline.com') {
    const tenant = url.pathname.split('/')[1]?.toLowerCase() ?? '';
    return MICROSOFT_OPEN_TENANTS.includes(tenant);
  }
  return PUBLIC_HOSTS.includes(host);
}
