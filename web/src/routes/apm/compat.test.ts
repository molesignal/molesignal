import { describe, expect, it } from 'vitest';

import {
  apmIndexTarget,
  legacyApmUserExperienceTarget,
  legacyRumSettingsTarget,
  legacyServicesTarget,
} from './compat';

describe('APM legacy route compatibility', () => {
  it.each([
    ['/apm/user-experience', '/rum/explorer'],
    ['/apm/user-experience/overview', '/rum/explorer'],
    ['/apm/user-experience/applications', '/rum/applications'],
    [
      '/apm/user-experience/sessions/view/session%2Fone',
      '/rum/sessions/view/session%2Fone',
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
        '/apm/user-experience/applications',
        '?app=shop',
        '#event-2',
      ),
    ).toBe(
      '/rum/applications?app=shop#event-2',
    );
    expect(legacyRumSettingsTarget('/rum/source-maps', '?app=shop')).toBe(
      '/rum/settings/source-maps?app=shop',
    );
    expect(legacyServicesTarget('/services/checkout', '?environment=prod')).toBe(
      '/apm/services/checkout?environment=prod',
    );
    expect(apmIndexTarget('?environment=prod')).toBe('/apm/overview?environment=prod');
  });
});
