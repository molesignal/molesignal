/** What is being done about error groups, and where they happened. */

export type ErrorIssueStatus = 'open' | 'investigating' | 'resolved' | 'ignored';

/** What is being done about an error group. */
export interface ErrorIssue {
  status: ErrorIssueStatus;
  assigneeId?: string | null;
  /** The platform team that has it. */
  teamId?: string | null;
  resolvedAt?: number | null;
  /** Resolved, and seen again since. */
  regressed: boolean;
  updatedBy: string;
  updatedAt: number;
}

/** Which error groups a list keeps, by what is being done about them. */
export type ErrorIssueFilter = 'all' | 'unresolved' | 'investigating' | 'resolved' | 'ignored';

/** Occurrences of one value: empty when the spans did not say it, `__other__` for the values past the most named. */
export interface ErrorShare {
  value: string;
  count: number;
}

/** Where errors happened: occurrences by version, environment and the services' language. */
export interface ErrorBreakdown {
  versions: ErrorShare[];
  environments: ErrorShare[];
  languages: ErrorShare[];
}
