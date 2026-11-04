import type { StatusPage, StatusPageDomainState } from '@/api/statusPages';

type PublicAddressPage = Pick<StatusPage, 'slug' | 'customDomain'>;

export function publicStatusPageUrl(
  page: PublicAddressPage,
  domainState?: StatusPageDomainState | null,
): string {
  if (page.customDomain && domainState === 'active') {
    return `https://${page.customDomain}/`;
  }
  return `/status/${encodeURIComponent(page.slug)}`;
}
