import type { StatusPage, StatusPageInput } from '@/api/statusPages';

export function statusPageInput(
  page: StatusPage,
  patch: Partial<StatusPageInput> = {},
): StatusPageInput {
  return {
    name: page.name,
    slug: page.slug,
    logo_url: page.logo_url,
    brand_color: page.brand_color,
    timezone: page.timezone,
    language: page.language,
    languages: page.languages,
    history_days: page.history_days,
    delivery_retention_days: page.delivery_retention_days,
    private_session_days: page.private_session_days,
    visibility: page.visibility,
    ...patch,
  };
}
