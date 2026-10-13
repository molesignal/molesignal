/** The formats a headless browser prints from the report page. */
export type PageFormat = 'pdf' | 'png';

/**
 * PDF and PNG deliveries are printed from the report page by a headless browser, which reads
 * the data with one user's access. The other formats (CSV, JSON, SVG) render no page and use
 * no user.
 */
export function rendersPage(format: string | undefined): format is PageFormat {
  return format === 'pdf' || format === 'png';
}

/** Whose access the deliveries of a report read data with. */
export type RunAs =
  /** The person looking at the report: it runs as them, or will once they save it. */
  | { kind: 'self' }
  /** Someone else, until the person looking at the report saves it. */
  | { kind: 'other'; userId: string }
  /** No one: the last save was not made by a signed-in user, so the deliveries fail. */
  | { kind: 'missing'; format: PageFormat };

/** What the backend holds for a report that has been saved. */
export interface SavedRunAs {
  /**
   * The format as the backend stores it, not as the editor shows it: the editor shows a
   * format it does not offer (SVG) as PDF, and what has been failing is the stored one.
   */
  format: string | undefined;
  /** `undefined` when the backend does not report the user at all. */
  userId: string | null | undefined;
}

/**
 * Whose access a report runs with, as seen by `currentUserId`.
 *
 * `format` is the format the next save gives the report: the one chosen in an editor, the
 * one shown in a list. Every save makes the saver the user the report runs as, so a report
 * that has not been saved yet runs as the person about to save it.
 *
 * Returns `null` when there is nothing to state: the format renders no page, or the backend
 * did not say who the report runs as.
 */
export function resolveRunAs({
  saved,
  format,
  currentUserId,
}: {
  saved: SavedRunAs | null;
  format: string;
  currentUserId: string;
}): RunAs | null {
  if (!rendersPage(format)) return null;
  if (!saved) return { kind: 'self' };
  if (saved.userId === undefined) return null;

  if (!saved.userId) {
    // A report that rendered no page until now never needed a user: nothing has failed.
    return rendersPage(saved.format)
      ? { kind: 'missing', format: saved.format }
      : { kind: 'self' };
  }

  return saved.userId === currentUserId
    ? { kind: 'self' }
    : { kind: 'other', userId: saved.userId };
}

/**
 * The name to show for the user a report runs as: the display name, the email of a user who
 * has not set one, and the id alone when the user no longer exists.
 */
export function runAsName(
  profile: { display_name: string; email: string } | null | undefined,
  userId: string,
): string {
  return profile?.display_name || profile?.email || userId;
}

/** Whether saving the report hands it over to the person who saves it. */
export function changesHandsOnSave(runAs: RunAs | null): boolean {
  return runAs !== null && runAs.kind !== 'self';
}
