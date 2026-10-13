/** The query parameter the report renderer puts on a page it is about to print. */
export const REPORT_RENDER_PARAM = 'report_render';

/** The query parameter naming the timezone the report is read in, when it has one of its own. */
export const REPORT_TIMEZONE_PARAM = 'report_tz';

/**
 * Whether this URL is being rendered for a report: a page for paper, with none
 * of the application around it.
 */
export function isReportRender(search: URLSearchParams): boolean {
  return search.get(REPORT_RENDER_PARAM) === '1';
}

/** The timezone the report asks for, or `null` to read it in the reader's own. */
export function reportTimezone(search: URLSearchParams): string | null {
  const timezone = search.get(REPORT_TIMEZONE_PARAM)?.trim();
  if (!timezone) return null;

  try {
    // Throws on a name the runtime does not know; a report in an unknown zone is read in the reader's.
    new Intl.DateTimeFormat('en-US', { timeZone: timezone });
    return timezone;
  } catch {
    return null;
  }
}
