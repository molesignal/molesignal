import { hasPermission, type ProductAccess } from '@/product/access';

/**
 * Whether the reader may open the search jobs page at all: the permission the
 * jobs API itself asks for.
 */
export function canUseSearchJobs(access: ProductAccess | null): boolean {
  return hasPermission('query.jobs.read', access) || hasPermission('sys.telemetry.read', access);
}

/**
 * Whether the reader is shown the jobs of everybody in the organization. A job
 * belongs to whoever submitted it; the backend lists and changes other
 * people's jobs only for an administrator, by the same two permissions.
 */
export function seesEverySearchJob(access: ProductAccess | null): boolean {
  return (
    hasPermission('org.running_queries.cancel', access) || hasPermission('sys.telemetry.manage', access)
  );
}
