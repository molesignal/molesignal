const STORAGE_KEY = 'molesignal-last-routes';
// Before the routes shared one key, each user + org pair had a key of its own.
const LEGACY_PREFIX = 'molesignal-last-route:';
const FALLBACK_HOME = '/home';
export const LAST_VISITED_HOME = 'last_visited';

/** `userId:orgId` → the last route visited as that user in that org. */
type LastRoutes = Record<string, string>;

function entryKey(userId: string, orgId: string): string {
  return `${userId}:${orgId}`;
}

function isRoute(value: unknown): value is string {
  return typeof value === 'string' && value.startsWith('/');
}

function readStored(): LastRoutes {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '{}');
    if (typeof parsed !== 'object' || parsed === null || Array.isArray(parsed)) return {};
    return Object.fromEntries(Object.entries(parsed).filter(([, route]) => isRoute(route)));
  } catch {
    return {};
  }
}

/** Folds the per-pair keys of older builds into the shared one, then removes them. */
function migrateLegacyKeys(): void {
  const legacyKeys: string[] = [];
  for (let index = 0; index < localStorage.length; index += 1) {
    const key = localStorage.key(index);
    if (key?.startsWith(LEGACY_PREFIX)) legacyKeys.push(key);
  }
  if (legacyKeys.length === 0) return;

  const routes = readStored();
  for (const key of legacyKeys) {
    const entry = key.slice(LEGACY_PREFIX.length);
    const route = localStorage.getItem(key);
    if (isRoute(route) && !(entry in routes)) routes[entry] = route;
    localStorage.removeItem(key);
  }
  localStorage.setItem(STORAGE_KEY, JSON.stringify(routes));
}

function readRoutes(): LastRoutes {
  migrateLegacyKeys();
  return readStored();
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
  const routes = readRoutes();
  const key = entryKey(userId, orgId);
  // Every navigation lands here; skip the write (and the storage event it
  // sends to other tabs) when nothing changed.
  if (routes[key] === route) return;
  routes[key] = route;
  localStorage.setItem(STORAGE_KEY, JSON.stringify(routes));
}

export function resolveDefaultHomeRoute(
  preference: string,
  userId: string,
  orgId: string,
): string {
  if (preference === LAST_VISITED_HOME) {
    return readRoutes()[entryKey(userId, orgId)] ?? FALLBACK_HOME;
  }
  return preference.startsWith('/') ? preference : FALLBACK_HOME;
}
