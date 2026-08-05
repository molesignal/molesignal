import * as licenseApi from './license';

export interface QuotaSnapshot {
  source: 'license';
  edition: string;
  ingest_bytes: number | null;
  ingest_limit_bytes: number | null;
  dashboards: number | null;
  dashboards_limit: number | null;
  alerts: number | null;
  alerts_limit: number | null;
  reset_at_micros: number | null;
}

export async function get(): Promise<QuotaSnapshot> {
  const license = await licenseApi.get();
  return {
    source: 'license',
    edition: license.edition,
    ingest_bytes: null,
    ingest_limit_bytes: license.max_ingest_bytes_per_day,
    dashboards: null,
    dashboards_limit: null,
    alerts: null,
    alerts_limit: null,
    reset_at_micros: license.expires_at_micros,
  };
}
