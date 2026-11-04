import type { StatusPage, StatusPageInput } from '@/api/statusPages';

export function statusPageInput(
  page: StatusPage,
  patch: Partial<StatusPageInput> = {},
): StatusPageInput {
  return {
    name: page.name,
    slug: page.slug,
    logoUrl: page.logoUrl,
    brandColor: page.brandColor,
    timezone: page.timezone,
    language: page.language,
    languages: page.languages,
    historyDays: page.historyDays,
    deliveryRetentionDays: page.deliveryRetentionDays,
    privateSessionDays: page.privateSessionDays,
    visibility: page.visibility,
    ...patch,
  };
}
