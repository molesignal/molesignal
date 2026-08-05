const LAST_ROUTE_PREFIX = 'molesignal-last-route';
const FALLBACK_HOME = '/home';
export const LAST_VISITED_HOME = 'last_visited';

function storageKey(userId: string, orgId: string): string {
  return `${LAST_ROUTE_PREFIX}:${userId}:${orgId}`;
}

export function rememberLastVisitedRoute(
  userId: string,
  orgId: string,
  route: string,
): void {
  if (!userId || !orgId || !route.startsWith('/')) return;
  if (
    route === '/' ||
    route.startsWith('/signin') ||
    route.startsWith('/signup') ||
    route.startsWith('/account/settings')
  ) {
    return;
  }
  localStorage.setItem(storageKey(userId, orgId), route);
}

export function resolveDefaultHomeRoute(
  preference: string,
  userId: string,
  orgId: string,
): string {
  if (preference === LAST_VISITED_HOME) {
    const stored = localStorage.getItem(storageKey(userId, orgId));
    return stored?.startsWith('/') ? stored : FALLBACK_HOME;
  }
  return preference.startsWith('/') ? preference : FALLBACK_HOME;
}
