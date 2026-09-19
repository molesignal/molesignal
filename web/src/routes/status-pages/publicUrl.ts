import type { StatusPage, StatusPageDomainState } from '@/api/statusPages';

type PublicAddressPage = Pick<StatusPage, 'slug' | 'custom_domain'>;

export function publicStatusPageUrl(
  page: PublicAddressPage,
  domainState?: StatusPageDomainState | null,
): string {
  if (page.custom_domain && domainState === 'active') {
    return `https://${page.custom_domain}/`;
  }
  return `/status/${encodeURIComponent(page.slug)}`;
}
