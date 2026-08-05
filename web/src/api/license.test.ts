import { afterEach, describe, expect, it, vi } from 'vitest';

import { http } from '@/lib/http';

import { get, type LicenseSnapshot, type LicenseUploadInput, upload } from './license';

const snapshot: LicenseSnapshot = {
  edition: 'community',
  verified: false,
  expired: false,
  issued_to: '',
  features: [],
  max_ingest_bytes_per_day: null,
  expires_at_micros: null,
  active_version_id: null,
};

describe('system License API', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('reads the active snapshot from the system-scoped endpoint', async () => {
    const request = vi.spyOn(http, 'get').mockResolvedValue({ data: snapshot } as never);

    await expect(get()).resolves.toEqual(snapshot);
    expect(request).toHaveBeenCalledWith('/system/license');
  });

  it('uploads an immutable version through the system-scoped versions endpoint', async () => {
    const input: LicenseUploadInput = {
      payload_b64: 'cGF5bG9hZA==',
      signature_b64: 'c2lnbmF0dXJl',
    };
    const request = vi.spyOn(http, 'post').mockResolvedValue({ data: snapshot } as never);

    await expect(upload(input)).resolves.toEqual(snapshot);
    expect(request).toHaveBeenCalledWith('/system/license/versions', input);
  });
});
