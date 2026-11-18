import type { NotifyActionLink, NotifyActionRefusal } from '@/api/notify/actions';
import { SUPPORTED_LOCALES, type Locale } from '@/i18n';
import type { ApiError } from '@/lib/http';
import type { PillTone } from '@/shell/chrome';

const REFUSALS: ReadonlySet<string> = new Set<NotifyActionRefusal>(['expired', 'used', 'inactive', 'no_access']);

/** The language the page is read in: the notification's, else the browser's. */
export function pageLanguage(language: string | undefined, browser: Locale): Locale {
  const asked = language?.toLowerCase();
  return SUPPORTED_LOCALES.find((locale) => locale === asked) ?? browser;
}

/** Why the server did not act, from its answer; `null` for a failure that may pass if tried again. A
 * link the server does not know is one that expired. */
export function refusalOf(error: ApiError): NotifyActionRefusal | null {
  if (error.code !== undefined && REFUSALS.has(error.code)) return error.code as NotifyActionRefusal;
  return error.status === 404 ? 'expired' : null;
}

/** Why a link cannot act any more, seen before it is pressed: it was used, or the alert is over. */
export function settledRefusal(link: NotifyActionLink): NotifyActionRefusal | null {
  if (link.usedAt != null) return 'used';
  return link.alert.status === 'resolved' || link.alert.status === 'closed' ? 'inactive' : null;
}

/** The colour of an alert's severity, as the alert pages give it. */
export function severityTone(severity: string): PillTone {
  if (severity === 'critical') return 'red';
  if (severity === 'error') return 'orange';
  if (severity === 'warning') return 'yellow';
  return severity === 'info' ? 'blue' : 'neutral';
}
