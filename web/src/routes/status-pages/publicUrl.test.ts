import { describe, expect, it } from 'vitest';

import { publicStatusPageUrl } from './publicUrl';

describe('publicStatusPageUrl', () => {
  it('keeps the slug path on the MoleSignal domain', () => {
    expect(
      publicStatusPageUrl({ slug: 'acme-cloud', customDomain: null }),
    ).toBe('/status/acme-cloud');
  });

  it('uses the site root only after the custom domain is active', () => {
    expect(
      publicStatusPageUrl({
        slug: 'acme-cloud',
        customDomain: 'status.acme.example',
      }, 'active'),
    ).toBe('https://status.acme.example/');
  });

  it('keeps the platform URL while domain verification or TLS is pending', () => {
    expect(
      publicStatusPageUrl({
        slug: 'acme-cloud',
        customDomain: 'status.acme.example',
      }, 'provisioning_tls'),
    ).toBe('/status/acme-cloud');
  });
});
