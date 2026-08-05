import { describe, expect, it } from 'vitest';

import {
  apmIndexTarget,
  legacyApmUserExperienceTarget,
  legacyRumSettingsTarget,
  legacyServicesTarget,
  legacyVersionCompareTarget,
} from './compat';

describe('APM legacy route compatibility', () => {
  it.each([
    ['/apm/user-experience', '/rum/overview'],
    ['/apm/user-experience/overview', '/rum/overview'],
    [
      '/apm/user-experience/sessions/view/session%2Fone',
      '/rum/sessions/view/session%2Fone',
    ],
    [
      '/apm/user-experience/errors/view/fingerprint',
      '/rum/errors/view/fingerprint',
    ],
    [
      '/apm/user-experience/performance/web-vitals',
      '/rum/performance/web-vitals',
    ],
    ['/apm/user-experience/source-maps', '/rum/settings/source-maps'],
    [
      '/apm/user-experience/upload-source-maps',
      '/rum/settings/source-maps/upload',
    ],
  ])('maps %s while preserving the complete suffix', (legacy, canonical) => {
    expect(legacyApmUserExperienceTarget(legacy)).toBe(canonical);
  });

  it('preserves query strings and hashes', () => {
    expect(
      legacyApmUserExperienceTarget(
        '/apm/user-experience/sessions',
        '?app=shop',
        '#event-2',
      ),
    ).toBe(
      '/rum/sessions?app=shop#event-2',
    );
    expect(legacyRumSettingsTarget('/rum/source-maps', '?app=shop')).toBe(
      '/rum/settings/source-maps?app=shop',
    );
    expect(legacyServicesTarget('/services/checkout', '?environment=prod')).toBe(
      '/apm/services/checkout?environment=prod',
    );
    expect(apmIndexTarget('?service=checkout')).toBe(
      '/apm/overview?service=checkout',
    );
    expect(
      legacyVersionCompareTarget(
        '?service=checkout&baseline=1.0&candidate=2.0',
      ),
    ).toBe('/apm/deployments?service=checkout&baseline=1.0&candidate=2.0');
  });
});
