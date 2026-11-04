import { describe, expect, it } from 'vitest';

import { rumDashboardPath } from './DashboardLink';

describe('the dashboard of an application', () => {
  it('is the browser one for a browser application, with the application and environment chosen', () => {
    expect(rumDashboardPath('browser', 'acme', 'shop-web', 'prod')).toBe(
      '/dashboards/rum-browser-acme?var-application=shop-web&var-environment=prod',
    );
  });

  it('chooses no environment where the page is of all of them', () => {
    expect(rumDashboardPath('browser', 'acme', 'shop-web', '')).toBe(
      '/dashboards/rum-browser-acme?var-application=shop-web',
    );
  });

  it('is the mobile one for any platform that runs on a phone', () => {
    for (const platform of ['android', 'ios', 'flutter', 'react-native'] as const) {
      expect(rumDashboardPath(platform, 'acme', 'shop-app', 'prod')).toBe(
        '/dashboards/rum-mobile-acme?var-application=shop-app&var-environment=prod',
      );
    }
  });

  it('encodes what is not safe in an address', () => {
    expect(rumDashboardPath('browser', 'acme', 'a b&c', '全部')).toBe(
      '/dashboards/rum-browser-acme?var-application=a+b%26c&var-environment=%E5%85%A8%E9%83%A8',
    );
  });

  it('is none for a platform the dashboards have nothing for, or without an organization', () => {
    expect(rumDashboardPath('other', 'acme', 'x', '')).toBeNull();
    expect(rumDashboardPath('browser', '', 'x', '')).toBeNull();
  });
});
