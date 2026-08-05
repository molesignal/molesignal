import { http } from '@/lib/http';

export interface LicenseSnapshot {
  edition: 'community' | 'pro' | string;
  verified: boolean;
  expired: boolean;
  issued_to: string;
  features: string[];
  max_ingest_bytes_per_day: number | null;
  expires_at_micros: number | null;
  active_version_id: string | null;
}

export interface LicenseUploadInput {
  /** Base64-encoded license payload (license body bytes, not the wrapper). */
  payload_b64: string;
  /** Base64-encoded detached signature over the payload. */
  signature_b64: string;
}

const SYSTEM_LICENSE_PATH = '/system/license';

export async function get(): Promise<LicenseSnapshot> {
  const { data } = await http.get<LicenseSnapshot>(SYSTEM_LICENSE_PATH);
  return data;
}

/**
 * Uploads and activates an immutable signed License version in `_sys`.
 * Invalid or expired packages are rejected; a successful response contains
 * the newly active snapshot so the page needs no follow-up GET.
 */
export async function upload(input: LicenseUploadInput): Promise<LicenseSnapshot> {
  const { data } = await http.post<LicenseSnapshot>(
    `${SYSTEM_LICENSE_PATH}/versions`,
    input,
  );
  return data;
}
