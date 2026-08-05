export interface FullscreenDashboardSelection {
  dashboardId: string;
  title: string;
  setAt: number;
}

const KEY_PREFIX = 'molesignal-fullscreen-dashboard';

function storageKey(orgId: string): string {
  return KEY_PREFIX + ':' + orgId;
}

export function getFullscreenDashboard(orgId: string | null | undefined): FullscreenDashboardSelection | null {
  if (!orgId || typeof window === 'undefined') return null;
  const raw = window.localStorage.getItem(storageKey(orgId));
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as Partial<FullscreenDashboardSelection>;
    if (typeof parsed.dashboardId !== 'string' || parsed.dashboardId.length === 0) return null;
    return {
      dashboardId: parsed.dashboardId,
      title: typeof parsed.title === 'string' ? parsed.title : parsed.dashboardId,
      setAt: typeof parsed.setAt === 'number' ? parsed.setAt : 0,
    };
  } catch {
    return null;
  }
}

export function setFullscreenDashboard(orgId: string, selection: FullscreenDashboardSelection): void {
  if (typeof window === 'undefined') return;
  window.localStorage.setItem(storageKey(orgId), JSON.stringify(selection));
}
