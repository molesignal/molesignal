import { describe, expect, it } from 'vitest';

import { buildStaticActions } from '@/palette/registry';
import type { TimeWindow } from '@/stores/useTimeStore';

function actions(opts: Partial<Parameters<typeof buildStaticActions>[0]> = {}) {
  return buildStaticActions({
    setTimeWindow: (_w: TimeWindow) => undefined,
    toggleTheme: () => undefined,
    toggleDensity: () => undefined,
    pinAnchor: () => undefined,
    copyInvestigationLink: () => undefined,
    openHelp: () => undefined,
    signOut: () => undefined,
    t: (key) => key,
    tNav: (key) => key,
    ...opts,
  });
}

describe('palette registry', () => {
  it('includes IA routes and onboarding creation actions', () => {
    const ids = actions().map((item) => item.id);

    expect(ids).toContain('route:home');
    expect(ids).toContain('route:logs');
    expect(ids).toContain('create:dashboard');
    expect(ids).toContain('onboarding:datasource');
  });



  it('removes routes and creation commands rejected by route access', () => {
    const items = actions({
      canAccessPath: (path) =>
        path !== '/settings/client_ip' &&
        path !== '/dashboards' &&
        path !== '/dashboards/new/edit' &&
        path !== '/pipelines/new',
    });
    const ids = items.map((item) => item.id);

    expect(ids).not.toContain('route:settings-client_ip');
    expect(ids).not.toContain('create:dashboard');
    expect(ids).not.toContain('create:pipeline');
    expect(ids).not.toContain('goto:dashboards');
    expect(ids).toContain('route:home');
  });

  it('boosts route-context actions', () => {
    const item = actions({ currentPath: '/dashboards' }).find((candidate) => candidate.id === 'create:dashboard');

    expect(item?.priority).toBeLessThan(0);
  });

  // ── The three previously-uncovered palette scenarios ──

  it('empty-org ordering: connect-source is the top-boosted action on the /home landing', () => {
    const items = actions({ currentPath: '/home' });
    const onboarding = items.find((i) => i.id === 'onboarding:datasource');
    const createDashboard = items.find((i) => i.id === 'create:dashboard');
    const createAlert = items.find((i) => i.id === 'create:alert');

    // On the empty-org landing, "connect a data source" is the strongest boost
    // (lower priority number = higher) — ahead of every other creation action.
    expect(onboarding?.priority).toBe(-30);
    expect(onboarding!.priority!).toBeLessThan(createDashboard!.priority ?? 0);
    expect(onboarding!.priority!).toBeLessThan(createAlert!.priority ?? 0);

    // Off the home landing it drops back to the baseline boost.
    const offHome = actions({ currentPath: '/logs' }).find((i) => i.id === 'onboarding:datasource');
    expect(offHome?.priority).toBe(-2);
  });

  it('dashboard-detail actions: create-dashboard is boosted on a /dashboards/:id route', () => {
    const onDetail = actions({ currentPath: '/dashboards/sample-id' }).find(
      (i) => i.id === 'create:dashboard',
    );
    const offDashboards = actions({ currentPath: '/logs' }).find((i) => i.id === 'create:dashboard');

    // Detail route still matches the `/dashboards` prefix → strong boost.
    expect(onDetail?.priority).toBe(-20);
    expect(offDashboards?.priority).toBe(-4);
  });


});
